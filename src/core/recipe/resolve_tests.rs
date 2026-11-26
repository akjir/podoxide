// Podoxide
// Copyright (C) 2026  Stefan Stark
//
// This program is free software: you can redistribute it and/or modify it under
// the terms of the GNU General Public License as published by the Free Software
// Foundation, either version 3 of the License, or (at your option)
// any later version.
//
// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
// FOR A PARTICULAR PURPOSE.  See the GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along with
// this program.  If not, see <https://www.gnu.org/licenses/>.

//! Unit tests for `.pod` recipe file loading, path checks, and container target resolution in [`super`].

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::core::config::Config;
use crate::core::container::{Container, VolumeMount};
use crate::error::{PodError, RecipeErrorKind};
use crate::io::system::fs::{self as system_fs, TestTempDir};
use super::{Recipe, parser, validation};

#[test]
fn load_root_recipe_pod_and_temp_recipes() {
    let config = Config::load(Path::new("./pods.conf")).unwrap();
    let root_recipe = Recipe::inspect(&config, "recipe").unwrap();

    assert_eq!(root_recipe.pod.id, "example-stack");
    assert_eq!(root_recipe.pod.name, "Example Web Stack");
    assert_eq!(
        root_recipe.pod.description,
        "A minimal but complete example recipe with a web app and database."
    );
    assert_eq!(root_recipe.pod.path, PathBuf::from("./pods/example-stack"));
    assert_eq!(root_recipe.pod.registry, "docker.io");
    assert_eq!(root_recipe.pod.publish, vec![String::from("8080:80/TCP")]);
    assert_eq!(
        root_recipe.pod.options,
        vec![String::from("--userns=keep-id")]
    );

    assert_eq!(root_recipe.pod.commands.len(), 3);
    let backup_command = root_recipe.pod.commands.get("backup").unwrap();
    assert_eq!(backup_command.name, "backup");
    assert_eq!(backup_command.description, "Create a database backup.");
    assert_eq!(backup_command.container, "*db");
    assert_eq!(backup_command.user.as_deref(), Some("dbuser"));
    assert_eq!(
        backup_command.execute,
        "db-dump -U dbuser -d app -f /var/lib/db/data/backup.sql"
    );

    let clear_cache_command = root_recipe.pod.commands.get("clear_cache").unwrap();
    assert_eq!(clear_cache_command.name, "clear_cache");
    assert_eq!(
        clear_cache_command.description,
        "Clear the web application cache."
    );
    assert_eq!(clear_cache_command.container, "*app");
    assert_eq!(clear_cache_command.user, None);
    assert_eq!(clear_cache_command.execute, "app-cli cache:clear");

    let migrate_command = root_recipe.pod.commands.get("migrate").unwrap();
    assert_eq!(migrate_command.name, "migrate");
    assert_eq!(migrate_command.description, "Run database migrations.");
    assert_eq!(migrate_command.container, "*db");
    assert_eq!(migrate_command.user.as_deref(), Some("dbuser"));
    assert_eq!(
        migrate_command.execute,
        "db-cli -U dbuser -d app -f /migrations/run.sql"
    );

    assert_eq!(root_recipe.containers.len(), 2);
    let db_container = &root_recipe.containers[0];
    assert_eq!(db_container.name, "example-stack-db");
    assert!(matches!(db_container.restart.as_str(), "always" | "never"));
    assert_eq!(db_container.registry, "docker.io");
    assert_eq!(db_container.image, "example/db-server:latest");
    assert_eq!(
        db_container.volumes,
        vec![VolumeMount {
            container_dir: String::from("/var/lib/db/data"),
            host_dir: String::from("data"),
            options: String::from("Z"),
        }]
    );
    assert_eq!(
        db_container.options,
        vec![String::from("--env DB_PASSWORD=secret")]
    );

    let app_container = &root_recipe.containers[1];
    assert_eq!(app_container.name, "example-stack-app");
    assert!(matches!(app_container.restart.as_str(), "always" | "never"));
    assert_eq!(app_container.image, "example/example-app:latest");
    assert_eq!(
        app_container.commands,
        vec![String::from("start-server.sh")]
    );
    assert_eq!(
        root_recipe.resolve_container("*app").unwrap().name,
        "example-stack-app"
    );
    assert_eq!(
        root_recipe.resolve_container_name("*app").unwrap(),
        "example-stack-app"
    );
    assert_eq!(
        root_recipe.resolve_container("1").unwrap().name,
        "example-stack-db"
    );
    assert_eq!(
        root_recipe.resolve_container_name("1").unwrap(),
        "example-stack-db"
    );

    let temp_dir = TestTempDir::new("recipe_test");

    let missing_error = parser::load(&temp_dir, "missing_recipe").unwrap_err();
    match &missing_error {
        PodError::Recipe(error) => assert_eq!(error.kind, RecipeErrorKind::NotFound),
        other => panic!("expected NotFound, got {other:?}"),
    }
    assert!(
        missing_error
            .to_string()
            .contains("Recipe error: recipe file not found:")
    );
    assert!(!Recipe::file_exists(&temp_dir, "missing_recipe"));
    let missing_path = temp_dir.join("missing_recipe.pod");
    assert!(matches!(
        Recipe::read_file(&missing_path),
        Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::NotFound
    ));
    let dir_as_recipe = temp_dir.join("dir_as_recipe.pod");
    fs::create_dir_all(&dir_as_recipe).unwrap();
    assert!(matches!(
        Recipe::read_file(&dir_as_recipe),
        Err(PodError::Io(_))
    ));

    let non_utf8_recipe_path = PathBuf::from(OsString::from_vec(vec![
        b'/', b't', 0xFF, b'.', b'p', b'o', b'd',
    ]));
    for invalid_read_path in [
        temp_dir.as_ref(),
        Path::new(""),
        Path::new("."),
        Path::new(".."),
        Path::new("../escape.pod"),
        Path::new("/tmp/../escape.pod"),
        Path::new("/etc/passwd"),
        Path::new(".pod"),
        Path::new("/tmp/.pod"),
        Path::new("..pod"),
        Path::new(".hidden.pod"),
        Path::new("app.extra.pod"),
        Path::new("-flag.pod"),
        Path::new("/tmp/-flag.pod"),
        Path::new("/bad:dir/app.pod"),
        Path::new("/tmp/app.pod/"),
        Path::new(" /tmp/app.pod"),
        Path::new("/tmp/app.pod "),
        Path::new("/tmp/app\n.pod"),
        Path::new("/tmp/\u{202E}app.pod"),
        non_utf8_recipe_path.as_path(),
    ] {
        let invalid_read_error = Recipe::read_file(invalid_read_path).unwrap_err();
        match invalid_read_error {
            PodError::Recipe(error) => assert_eq!(error.kind, RecipeErrorKind::InvalidPath),
            other => {
                panic!("expected InvalidPath for '{invalid_read_path:?}', got {other:?}")
            }
        }
    }

    for invalid_name in [
        "../escape",
        "",
        ".",
        "..",
        ".pod",
        "..pod",
        ".hidden.pod",
        "app.extra.pod",
    ] {
        assert!(!Recipe::file_exists(&temp_dir, invalid_name));
        let invalid_target_error = parser::load(&temp_dir, invalid_name).unwrap_err();
        match invalid_target_error {
            PodError::Recipe(error) => assert_eq!(error.kind, RecipeErrorKind::InvalidPath),
            other => panic!("expected InvalidPath for '{invalid_name}', got {other:?}"),
        }
    }

    for invalid_dir in [Path::new("../escape"), Path::new("/bad:dir")] {
        assert!(!Recipe::file_exists(invalid_dir, "webapp"));
        let invalid_dir_error = parser::load(invalid_dir, "webapp").unwrap_err();
        match invalid_dir_error {
            PodError::Recipe(error) => assert_eq!(error.kind, RecipeErrorKind::InvalidPath),
            other => panic!("expected InvalidPath for '{invalid_dir:?}', got {other:?}"),
        }
    }

    let valid_path = temp_dir.join("webapp.pod");
    let webapp_content = "\
pod \"  Si  Po \" {
    name \" Simple Pod  \"
    registry registry.io
    publish 8433 433
    publish 8080 80 TCP
    publish \"127.0.0.1:\" 42
    publish 127.0.0.1:62 43 UDP
    publish 500-600
    publish \"\" 83
    publish 124
    publish 12 UDP
    option --network slirp4netns:port_handler=slirp4netns
    command migrate {
        container *db
        execute \"sh migrate.sh\"
    }
}
container \" supR App \" {
    restart never
    image alpine:latest
    volume \"\" /container/dir/anonymous \"\"
    volume named_volume /container/dir/named ro
    volume /absolute/path /container/dir/absolute
    volume ./relative/path /container/dir/relative z
    option --env \"FOO=bar\"
    command \"O'Hara\"
    command \"Hancock 2\"
}
container *db {
    registry db.io
    image postgres:16
}
container {
    image redis:7
}
";
    system_fs::write_file(&valid_path, webapp_content).unwrap();
    assert!(Recipe::file_exists(&temp_dir, "webapp"));
    assert!(Recipe::file_exists(&temp_dir, "webapp.pod"));
    assert_eq!(Recipe::read_file(&valid_path).unwrap(), webapp_content);

    let (mut loaded, _, _) = parser::load(&temp_dir, "webapp.pod").unwrap();
    let mut custom_config = Config::new();
    custom_config.pods.path = PathBuf::from("/pods");
    validation::validate_recipe_inner(&mut loaded, &custom_config, "webapp").unwrap();

    assert_eq!(loaded.pod.id, "si_po");
    assert_eq!(loaded.pod.name, "Simple Pod");
    assert_eq!(loaded.pod.path, PathBuf::from("/pods/si_po"));
    assert_eq!(
        loaded.pod.publish,
        vec![
            "8433:433",
            "8080:80/TCP",
            "127.0.0.1::42",
            "127.0.0.1:62:43/UDP",
            "500-600",
            "83",
            "124",
            "12/UDP",
        ]
    );
    assert_eq!(
        loaded.pod.options,
        vec!["--network", "slirp4netns:port_handler=slirp4netns"]
    );
    assert!(loaded.pod.commands.contains_key("migrate"));

    assert_eq!(loaded.containers.len(), 3);
    assert_eq!(loaded.containers[0].name, "supr_app");
    assert_eq!(loaded.containers[0].volumes.len(), 4);
    assert_eq!(loaded.containers[0].options, vec!["--env", "FOO=bar"]);
    assert_eq!(loaded.containers[0].commands, vec!["O'Hara", "Hancock 2"]);
    assert_eq!(loaded.containers[1].name, "si_po-db");
    assert_eq!(loaded.containers[1].registry, "db.io");
    assert_eq!(loaded.containers[2].name, "si_po-3");

    assert_eq!(loaded.resolve_container_name("1").unwrap(), "supr_app");
    assert_eq!(loaded.resolve_container_name(" 1 ").unwrap(), "supr_app");
    assert_eq!(loaded.resolve_container_name("2").unwrap(), "si_po-db");
    assert_eq!(loaded.resolve_container_name("3").unwrap(), "si_po-3");
    assert_eq!(
        loaded.resolve_container_name("SuPr ApP").unwrap(),
        "supr_app"
    );
    assert_eq!(loaded.resolve_container_name("*db").unwrap(), "si_po-db");
    assert_eq!(loaded.resolve_container_name("db").unwrap(), "si_po-db");

    for unknown in [
        "",
        "   ",
        "0",
        "+1",
        "4",
        "*",
        "*-bad",
        "*missing",
        "missing",
        "bad\nname",
    ] {
        let error = loaded.resolve_container_name(unknown).unwrap_err();
        assert_eq!(error.kind, RecipeErrorKind::ContainerNotFound);
        assert_eq!(
            error.line, None,
            "CLI target mismatch {unknown:?} must not blame pod line"
        );
    }
}

#[test]
fn resolve_container_rejects_empty_wildcard_and_handles_unvalidated_recipes() {
    let mut resolve_recipe = Recipe::new();
    resolve_recipe.has_pod = true;
    resolve_recipe.pod.id = String::from("demo");
    let mut dash_container = Container::new();
    dash_container.name = String::from("demo-");
    dash_container.image = String::from("alpine:latest");
    resolve_recipe.containers.push(dash_container);

    let wildcard_err = resolve_recipe.resolve_container("*").unwrap_err();
    assert_eq!(wildcard_err.kind, RecipeErrorKind::ContainerNotFound);
    assert_eq!(wildcard_err.line, None);
    assert_eq!(
        resolve_recipe.resolve_container(" 1 ").unwrap().name,
        resolve_recipe.resolve_container("1").unwrap().name
    );

    let unvalidated_no_pod =
        Recipe::from_str("container *f {\n    image alpine:latest\n}\n").unwrap();
    let resolve_error = unvalidated_no_pod.resolve_container_name("1").unwrap_err();
    assert_eq!(resolve_error.kind, RecipeErrorKind::ContainerNotFound);
    assert_eq!(resolve_error.line, None);
    assert!(!resolve_error.to_string().contains("line 0:"));

    let mut invalid_pod_recipe = unvalidated_no_pod;
    invalid_pod_recipe.has_pod = true;
    invalid_pod_recipe.pod.id = String::from("-flag");
    invalid_pod_recipe.pod.line = 3;
    let invalid_pod_resolve = invalid_pod_recipe.resolve_container_name("*f").unwrap_err();
    assert_eq!(invalid_pod_resolve.kind, RecipeErrorKind::ContainerNotFound);
    assert_eq!(invalid_pod_resolve.line, Some(3));

    let mut numeric_recipe = Recipe::from_str(
        "\
pod num_pod {
    name \"Numeric Pod\"
    path /pods/num_pod
    registry docker.io
}
container 0 {
    image alpine:latest
}
container 8080 {
    image alpine:latest
}
container *10 {
    image alpine:latest
}
",
    )
    .unwrap();
    let mut numeric_config = Config::new();
    numeric_config.pods.path = PathBuf::from("/pods");
    validation::validate_recipe_inner(&mut numeric_recipe, &numeric_config, "num_pod").unwrap();

    assert_eq!(numeric_recipe.resolve_container_name("0").unwrap(), "0");
    assert_eq!(
        numeric_recipe.resolve_container_name("8080").unwrap(),
        "8080"
    );
    assert_eq!(
        numeric_recipe.resolve_container_name("10").unwrap(),
        "num_pod-10"
    );
    assert_eq!(numeric_recipe.resolve_container_name("1").unwrap(), "0");
    assert_eq!(
        numeric_recipe.resolve_container_name("4").unwrap_err().kind,
        RecipeErrorKind::ContainerNotFound
    );
}
