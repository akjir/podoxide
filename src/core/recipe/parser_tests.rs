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

//! Unit tests for `.pod` recipe parsing, quoting variants, and multi-container command rendering in [`super`].

use std::path::PathBuf;
use std::str::FromStr;

use crate::core::config::Config;
use crate::core::container::{self, Container};
use crate::core::pod::Pod;
use crate::error::{PodError, PslErrorKind, RecipeErrorKind};
use super::{Recipe, validation};

fn assert_recipe_error(input: &str, expected_kind: RecipeErrorKind) {
    let error = Recipe::from_str(input).unwrap_err();
    match error {
        PodError::Recipe(recipe_error) => {
            assert_eq!(recipe_error.kind, expected_kind, "input: {input}");
            let diagnostic = recipe_error.diagnostic.as_ref().unwrap();
            assert_eq!(diagnostic.file_path, "<input>");
        }
        other => panic!("expected PodError::Recipe({expected_kind:?}), got {other:?}"),
    }
}

fn simulate_container_create(container: &Container, pod: &Pod) -> String {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    pod.ensure_created_with(true, |_| true, &mut stdout, &mut stderr)
        .unwrap();
    container::create_with(container, pod, true, &mut stdout, &mut stderr).unwrap();
    assert!(stderr.is_empty());
    String::from_utf8(stdout)
        .unwrap()
        .lines()
        .nth(1)
        .unwrap()
        .to_owned()
}

fn simulate_pod_create(pod: &Pod) -> String {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    pod.create_with(true, &mut stdout, &mut stderr).unwrap();
    assert!(stderr.is_empty());
    String::from_utf8(stdout)
        .unwrap()
        .lines()
        .nth(1)
        .unwrap()
        .to_owned()
}

#[test]
fn parse_and_build_args_for_all_option_quoting_variants() {
    let config = Config::new();
    for option_line in [
        "option   \"--health-cmd='redis-cli ping | grep -q PONG || exit 1'\"",
        "option   '--health-cmd=\"redis-cli ping | grep -q PONG || exit 1\"'",
        "option   --health-cmd='redis-cli ping | grep -q PONG || exit 1'",
        "option   --health-cmd=\"redis-cli ping | grep -q PONG || exit 1\"",
    ] {
        let input = format!(
            "\
pod buci {{
    name     \"Buci\"
    path     /pods/buci
    registry docker.io
}}
container *cache {{
    restart never
    image   valkey/valkey:9
    {option_line}
}}
"
        );
        let mut recipe = Recipe::from_str(&input).unwrap();
        validation::validate_recipe_inner(&mut recipe, &config, "buci").unwrap();
        assert_eq!(
            simulate_container_create(&recipe.containers[0], &recipe.pod),
            "podman create --name buci-cache --pod buci --restart never '--health-cmd=redis-cli ping | grep -q PONG || exit 1' docker.io/valkey/valkey:9"
        );
    }

    for option_line in [
        "option   --health-cmd 'redis-cli ping | grep -q PONG || exit 1'",
        "option   --health-cmd \"redis-cli ping | grep -q PONG || exit 1\"",
    ] {
        let input = format!(
            "\
pod buci {{
    name     \"Buci\"
    path     /pods/buci
    registry docker.io
}}
container *cache {{
    restart never
    image   valkey/valkey:9
    {option_line}
}}
"
        );
        let mut recipe = Recipe::from_str(&input).unwrap();
        validation::validate_recipe_inner(&mut recipe, &config, "buci").unwrap();
        assert_eq!(
            simulate_container_create(&recipe.containers[0], &recipe.pod),
            "podman create --name buci-cache --pod buci --restart never --health-cmd 'redis-cli ping | grep -q PONG || exit 1' docker.io/valkey/valkey:9"
        );
    }

    for invalid_psl_option in [
        "option   '--health-cmd='redis-cli ping | grep -q PONG || exit 1''",
        "option   \"--health-cmd=\"redis-cli ping | grep -q PONG || exit 1\"\"",
    ] {
        let input = format!(
            "pod buci {{\n    name \"Buci\"\n    registry docker.io\n}}\ncontainer *cache {{\n    image valkey/valkey:9\n    {invalid_psl_option}\n}}\n"
        );
        let error = Recipe::from_str(&input).unwrap_err();
        match error {
            PodError::Psl(psl_error) => {
                assert_eq!(psl_error.kind, PslErrorKind::UnexpectedToken);
                assert_eq!(psl_error.line, 7);
            }
            other => panic!("expected PslError(UnexpectedToken), got {other:?}"),
        }
    }

    for invalid_inner_quote in [
        "option   \"--health-cmd='redis-cli ping\"",
        "option   '--health-cmd=\"redis-cli ping'",
        "option   \"--health-cmd='redis-cli'extra\"",
        "option   \"--health-cmd 'redis-cli ping\"",
    ] {
        let input = format!(
            "pod buci {{\n    name \"Buci\"\n    registry docker.io\n}}\ncontainer *cache {{\n    image valkey/valkey:9\n    {invalid_inner_quote}\n}}\n"
        );
        assert_recipe_error(&input, RecipeErrorKind::InvalidOption);
    }
}

#[test]
fn parse_and_validate_multi_container_service_recipe_abstraction() {
    let input = "\
pod stack {
    name     \"Service Stack\"
    path     /srv/storage/stack
    registry registry.io
    publish  18080 80 TCP
    option   \"--network slirp4netns:port_handler=slirp4netns\"
    option   \"--sysctl net.ipv4.ip_unprivileged_port_start=80\"
    option   --userns=keep-id:uid=1000,gid=1000

    command sync_indices {
        description \"Synchronize database indices.\"
        container   *web
        user        1000
        execute     \"/srv/www/cli db:sync-indices\"
    }

    command repair_state {
        description \"Repair application state.\"
        container   *web
        user        1000
        execute     \"/srv/www/cli system:repair --include-slow\"
    }

    command scan_storage {
        description \"Scan storage files.\"
        container   *web
        user        1000
        execute     \"/srv/www/cli files:scan --all\"
    }
}

container *store {
    restart never
    image   kv-store:7-alpine
    volume  store /data Z
}

container *web {
    restart never
    image   web-service:20
    volume  web                 /srv/www        Z
    volume  config              /srv/www/config Z
    volume  files               /srv/www/files  Z
    volume  /srv/storage/shared /shared         z
    option  \"--user 1000:1000\"
}
";
    let mut recipe = Recipe::from_str(input).unwrap();
    let config = Config::new();
    validation::validate_recipe_inner(&mut recipe, &config, "stack").unwrap();

    assert_eq!(recipe.pod.id, "stack");
    assert_eq!(recipe.pod.name, "Service Stack");
    assert!(recipe.pod.description.is_empty());
    assert_eq!(recipe.pod.path, PathBuf::from("/srv/storage/stack"));
    assert_eq!(recipe.pod.registry, "registry.io");
    assert_eq!(recipe.pod.publish, vec!["18080:80/TCP"]);
    assert_eq!(
        recipe.pod.options,
        vec![
            "--network slirp4netns:port_handler=slirp4netns",
            "--sysctl net.ipv4.ip_unprivileged_port_start=80",
            "--userns=keep-id:uid=1000,gid=1000",
        ]
    );
    assert_eq!(
        simulate_pod_create(&recipe.pod),
        "podman pod create --name stack --publish 18080:80/TCP --network slirp4netns:port_handler=slirp4netns --sysctl net.ipv4.ip_unprivileged_port_start=80 '--userns=keep-id:uid=1000,gid=1000'"
    );

    assert_eq!(recipe.pod.commands.len(), 3);
    let sync_command = recipe.pod.commands.get("sync_indices").unwrap();
    assert_eq!(sync_command.container, "*web");
    assert_eq!(sync_command.user.as_deref(), Some("1000"));
    assert_eq!(sync_command.execute, "/srv/www/cli db:sync-indices");
    assert_eq!(
        recipe
            .resolve_container_name(&sync_command.container)
            .unwrap(),
        "stack-web"
    );

    assert_eq!(recipe.containers.len(), 2);
    assert_eq!(
        simulate_container_create(&recipe.containers[0], &recipe.pod),
        "podman create --name stack-store --pod stack --restart never --volume /srv/storage/stack/store:/data:Z registry.io/kv-store:7-alpine"
    );
    assert_eq!(
        simulate_container_create(&recipe.containers[1], &recipe.pod),
        "podman create --name stack-web --pod stack --restart never --volume /srv/storage/stack/web:/srv/www:Z --volume /srv/storage/stack/config:/srv/www/config:Z --volume /srv/storage/stack/files:/srv/www/files:Z --volume /srv/storage/shared:/shared:z --user 1000:1000 registry.io/web-service:20"
    );

    let nginx_input = "\
pod nginx {
    name     \"Nginx\"
    path     /pods/nginx
    registry docker.io
    publish  443 443 TCP
    publish  443 443 UDP
    # Use \"--network pasta\" on Podman >= 5.0
    option   \"--network slirp4netns:port_handler=slirp4netns\"
}

container *app {
    restart never
    image   nginx:stable-alpine
    volume  nginx.conf /etc/nginx/nginx.conf ro,Z
    volume  certs      /etc/nginx/certs      ro,Z
    volume  bots       /etc/nginx/bots.d     ro,Z
    volume  config     /etc/nginx/conf.d     ro,Z
    volume  log        /var/log/nginx        Z
}
";
    let mut nginx_recipe = Recipe::from_str(nginx_input).unwrap();
    validation::validate_recipe_inner(&mut nginx_recipe, &config, "nginx").unwrap();
    assert_eq!(
        simulate_pod_create(&nginx_recipe.pod),
        "podman pod create --name nginx --publish 443:443/TCP --publish 443:443/UDP --network slirp4netns:port_handler=slirp4netns"
    );
    assert_eq!(
        simulate_container_create(&nginx_recipe.containers[0], &nginx_recipe.pod),
        "podman create --name nginx-app --pod nginx --restart never --volume '/pods/nginx/nginx.conf:/etc/nginx/nginx.conf:ro,Z' --volume '/pods/nginx/certs:/etc/nginx/certs:ro,Z' --volume '/pods/nginx/bots:/etc/nginx/bots.d:ro,Z' --volume '/pods/nginx/config:/etc/nginx/conf.d:ro,Z' --volume /pods/nginx/log:/var/log/nginx:Z docker.io/nginx:stable-alpine"
    );
}
