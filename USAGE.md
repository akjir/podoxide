# Podoxide Usage Guide

This guide documents all implemented features, configuration schemas, recipe definitions, targeting rules, and command-line modes of Podoxide (`pods`).

## Table of Contents

- [Command Syntax](#command-syntax)
  - [Modes Overview](#modes-overview)
  - [Global Options](#global-options)
- [Configuration Reference](#configuration-reference)
- [Recipe Reference](#recipe-reference)
  - [Root Blocks](#root-blocks)
  - [Pod Fields](#pod-fields)
  - [Container Fields](#container-fields)
  - [Recipe Example](#recipe-example)
- [Targeting Rules](#targeting-rules)
  - [Recipe and Group Targets](#recipe-and-group-targets)
  - [Container Targets](#container-targets)
- [Default Mode](#default-mode)
  - [Actions](#actions)
  - [Options](#options)
  - [Usage](#usage)
- [Logs Mode (`log`)](#logs-mode-log)
  - [Actions](#actions-1)
  - [Options](#options-1)
  - [Usage](#usage-1)
- [Connect Mode (`con`)](#connect-mode-con)
  - [Actions](#actions-2)
  - [Options](#options-2)
  - [Usage](#usage-2)
- [Command Mode (`cmd`)](#command-mode-cmd)
  - [Actions](#actions-3)
  - [Options](#options-3)
  - [Usage](#usage-3)
- [Recipe Mode (`rec`)](#recipe-mode-rec)
  - [Actions](#actions-4)
  - [Options](#options-4)
  - [Usage](#usage-4)
- [Config Mode (`cfg`)](#config-mode-cfg)
  - [Actions](#actions-5)
  - [Options](#options-5)
  - [Usage](#usage-5)
- [Image Mode (`img`)](#image-mode-img)
  - [Actions](#actions-6)
  - [Options](#options-6)
  - [Usage](#usage-6)
- [Init Mode](#init-mode)
  - [Actions](#actions-7)
  - [Options](#options-7)
  - [Usage](#usage-7)

## Command Syntax

Invoke Podoxide with the compiled `pods` binary:

```bash
pods [MODE] [OPTIONS] [ACTION] [TARGETS...]
```

Argument parsing follows these rules:

- **Mode Resolution:** If the first positional argument matches a mode name (`command`, `config`, `connect`, `image`, `init`, `logs`, `recipe`) or its 3-letter alias (`cmd`, `cfg`, `con`, `img`, `log`, `rec`), that mode is selected. Otherwise, Podoxide selects the [Default Mode](#default-mode) and treats the first positional argument as the action.
- **Mode Aliases:** Every mode except `init` provides a 3-letter alias (`cmd`, `cfg`, `con`, `img`, `log`, `rec`) as an exact drop-in replacement for the first positional mode argument (for example, `pods cmd exec <recipe> <command>` is identical to `pods command exec <recipe> <command>`).
- **No Arguments:** Running `pods` without arguments executes `pods help`.
- **Option Placement:** Options prefixed with `--` can appear anywhere in the command line. Options that require a value (`--config`, `--since`, `--tail`, `--until`) accept both `--flag=<value>` and space-separated `--flag <value>` syntax.
- **Configuration Bypass:** The `init` mode, `pods version`, all `help` actions (`pods help`, `pods <mode> help`), and parameterless calls to `command`, `connect`, `image`, or `logs` (including their 3-letter aliases) run without loading `pods.conf`.
- **Privilege Check:** When executed with elevated privileges (`sudo` or `root`), Podoxide prints a warning and requires interactive confirmation (`yes`) before running.

### Modes Overview

| Mode              | Actions                                                                                                     | Target                            | Description                                                       |
| :---------------- | :---------------------------------------------------------------------------------------------------------- | :-------------------------------- | :---------------------------------------------------------------- |
| `(default)`       | `up`, `down`, `create`, `remove`, `start`, `stop`, `restart`, `reload`, `update`, `status`, `help`, `version` | `<recipe>[/<container>]`, `@<group>` | Manage pod and container lifecycles and inspect runtime status.   |
| `logs` (`log`)    | `show`, `follow`, `help`                                                                                    | `<recipe>[/<container>]`          | Display or follow logs for a pod or a specific container.         |
| `connect` (`con`) | `shell`, `help`                                                                                             | `<recipe>[/<container>]`          | Open an interactive shell inside a running container.             |
| `command` (`cmd`) | `list`, `exec`, `help`                                                                                      | `<recipe> [<command>\|<idx>]`     | List or execute maintenance commands defined in a recipe.         |
| `recipe` (`rec`)  | `list`, `inspect`, `show`, `edit`, `help`                                                                   | `[<recipe>]`                      | List, validate, display, or edit `.pod` recipe files.             |
| `config` (`cfg`)  | `inspect`, `show`, `edit`, `help`                                                                           | `-`                               | Validate, display, or edit the active configuration file.         |
| `image` (`img`)   | `update`, `prune`, `help`                                                                                   | `[<recipe>[/<container>], @<group>]` | Pull recipe container images or preview and remove unused images. |
| `init`            | `create`, `help`                                                                                            | `-`                               | Generate initial `pods.conf` and `recipe.pod` template files.     |

### Global Options

| Option            | Description                                                                              |
| :---------------- | :--------------------------------------------------------------------------------------- |
| `--config=<name>` | Load an alternative configuration file by name or path (with or without `.conf`).        |
| `--debug`         | Enable verbose debug output for argument parsing, path resolution, and target expansion. |
| `--simulate`      | Preview generated commands and file operations without executing them on the host.       |

## Configuration Reference

By default, Podoxide loads `./pods.conf`, written in the block-based PodScript Language (`PSL`). Use `--config=<name>` to load an alternative file (for example, `--config=staging` loads `./staging.conf`). Every top-level statement in `pods.conf` must be a root block (`general`, `pods`, or `recipes`).

| Block / Directive  | Type      | Default | Required | Description                                                                                 |
| :----------------- | :-------- | :------ | :------- | :------------------------------------------------------------------------------------------ |
| `general.editor`   | `string`  | `""`    | No       | External editor command executed by `pods config edit` and `pods recipe edit`.              |
| `general.simulate` | `boolean` | `true`  | No       | Enable dry-run simulation mode globally unless explicitly set to `false`.                   |
| `pods.path`        | `string`  | `""`    | No       | Default base directory for pod volume paths when a recipe omits `path` in its `pod` block.  |
| `recipes.path`     | `string`  | `"."`   | No       | Directory path where Podoxide searches for `<recipe>.pod` files.                            |
| `recipes.groups`   | `block`   | `{}`    | Yes      | Non-empty block mapping group names to one or more recipe names or `@<group>` entries.      |

> [!IMPORTANT]
> Every recipe must belong to at least one group in `recipes.groups` to be recognized by lifecycle commands, `pods image update`, and `recipe` mode actions.

```psl
general {
    editor   vim
    simulate false
}

pods {
    path /pods
}

recipes {
    path .

    groups {
        backend db-server cache-server
        all     @backend example-app
    }
}
```

## Recipe Reference

Each recipe is a `.pod` file (`<recipe>.pod`) in `recipes.path` written in the PodScript Language (`PSL`). Every top-level statement in a recipe file must be a root block (`pod` or `container`).

### Root Blocks

| Block                  | Cardinality | Required | Description                                                                                 |
| :--------------------- | :---------- | :------- | :------------------------------------------------------------------------------------------ |
| `pod <id> { ... }`     | `1`         | Yes      | Defines the pod identifier (`<id>`), recipe metadata, shared namespace settings, and ports. |
| `container [<id>] { }` | `1..N`      | Yes      | Defines a container in the pod. Processed sequentially in top-to-bottom declaration order.  |

### Pod Fields

The `pod <id> { ... }` block configures the Podman pod and recipe metadata:

| Field / Directive        | Type        | Required | Description                                                                                   |
| :----------------------- | :---------- | :------- | :-------------------------------------------------------------------------------------------- |
| `<id>`                   | `string`    | Yes      | Pod name in Podman (normalized to lowercase with underscores); specified in the block header. |
| `name`                   | `string`    | Yes      | Human-readable display name of the recipe.                                                    |
| `description`            | `string`    | No       | Summary displayed by `pods recipe list`.                                                      |
| `registry`               | `string`    | Yes      | Default image registry prefixed to container images that do not specify `container.registry`. |
| `path`                   | `string`    | Cond.    | Base host directory for relative volume mounts; defaults to `<pods.path>/<id>`.               |
| `publish`                | `directive` | No       | Port mapping arguments translated into `--publish` flags on `podman pod create` (repeatable). |
| `option`                 | `directive` | No       | Raw flag string appended directly to `podman pod create` (repeatable).                        |
| `command <name> { ... }` | `block`     | No       | Maintenance command sub-block executed via `pods command` (repeatable).                       |

#### Port Publishing (`publish`)

Each `publish` directive in the `pod` block maps to a `--publish` flag on `podman pod create`:

| Directive Format                       | Generated Flag                             |
| :------------------------------------- | :----------------------------------------- |
| `publish 8080` or `publish "" 8080`    | `--publish 8080`                           |
| `publish 8080 TCP` or `publish 53 UDP` | `--publish 8080/TCP` or `--publish 53/UDP` |
| `publish 8080 80`                      | `--publish 8080:80`                        |
| `publish 8080 80 TCP`                  | `--publish 8080:80/TCP`                    |
| `publish 127.0.0.1:8080 80 TCP`        | `--publish 127.0.0.1:8080:80/TCP`          |

#### Maintenance Commands (`command <name>`)

Each `command <name> { ... }` block inside `pod` defines a named maintenance command for `pods command`:

| Directive     | Type     | Required | Description                                                                             |
| :------------ | :------- | :------- | :-------------------------------------------------------------------------------------- |
| `container`   | `string` | Yes      | Target container by 1-based index (`1`), relative name (`*db` or `db`), or full name.   |
| `execute`     | `string` | Yes      | Command line executed inside the container via `podman exec -it`.                       |
| `user`        | `string` | No       | Username or UID passed to `podman exec -it -u <user>`.                                  |
| `description` | `string` | No       | Summary shown in `pods command list` (omitting triggers a warning when listing).        |

### Container Fields

Containers declared in `container [<id>] { ... }` blocks are created and started sequentially from top to bottom (`1` to `N`) and stopped and removed in reverse bottom-to-top order (`N` down to `1`).

| Field / Directive | Type        | Required | Description                                                                                 |
| :---------------- | :---------- | :------- | :------------------------------------------------------------------------------------------ |
| `<id>`            | `string`    | No       | Container name in the block header; supports relative (`*db`), explicit, or omitted naming. |
| `image`           | `string`    | Yes      | Image repository and tag (`example/db-server:latest`).                                      |
| `registry`        | `string`    | No       | Registry override for this container; falls back to `pod.registry`.                         |
| `restart`         | `string`    | No       | Restart policy passed to `--restart` (for example, `always` or `on-failure`).               |
| `volume`          | `directive` | No       | Volume mount definition `<host_dir> <container_dir> [<options>]` (repeatable).              |
| `option`          | `directive` | No       | Raw flag string appended directly to `podman create` (repeatable).                          |
| `command`         | `directive` | No       | Command and argument tokens appended after the image in `podman create` (repeatable).       |

#### Container Naming Rules

- **Omitted (`container { ... }`):** Expands to `<pod_id>-<index>` (for example, `example-stack-1`).
- **Prefixed with `*` (`container *db { ... }`):** Expands to `<pod_id>-<suffix>` (for example, `*db` becomes `example-stack-db`).
- **Without `*` (`container custom { ... }`):** Normalized (lowercase, spaces replaced with `_`) and used as the exact container name.

#### Volume Mount Rules (`volume`)

Each `volume` directive specifies a host source, container destination, and optional mount options (`volume <host_dir> <container_dir> [<options>]`):

| `host_dir` Value   | Resolution                    | Generated `--volume` Value                    |
| :----------------- | :---------------------------- | :-------------------------------------------- |
| `""`               | Anonymous volume              | `<container_dir>[:<options>]`                 |
| `data` or `./data` | Resolved against `pod.path`   | `<pod.path>/data:<container_dir>[:<options>]` |
| `/var/data`        | Absolute host path used as-is | `/var/data:<container_dir>[:<options>]`       |

### Recipe Example

```psl
pod example-stack {
    name        "Example Web Stack"
    description "A minimal but complete example recipe with a web app and database."
    path        /pods/example-stack
    registry    docker.io
    publish     8080 80 TCP
    option      --userns=keep-id

    command migrate {
        description "Run database migrations."
        container   *db
        user        dbuser
        execute     "db-cli -U dbuser -d app -f /migrations/run.sql"
    }
}

container *db {
    restart  always
    registry docker.io
    image    example/db-server:latest
    volume   data /var/lib/db/data Z # Mounts 'data' with SELinux label 'Z'
    option   "--env DB_PASSWORD=secret"
}

container *app {
    restart always
    image   example/example-app:latest
    command start-server.sh
}
```

## Targeting Rules

### Recipe and Group Targets

The [Default Mode](#default-mode) and `pods image update` accept one or more recipe names (`<recipe>`) or group references (`@<group>`):

- **Sequential Order:** Recipes execute in the exact order they appear after group expansion.
- **First Appearance Rule:** When a recipe appears multiple times across arguments or nested `@<group>` definitions, only its first occurrence is executed and subsequent duplicates are removed.
- **Group Validation:** Every targeted recipe must exist in at least one group in `pods.conf`. Container suffixes (`@group/container`) are not valid on groups.

### Container Targets

The [Default Mode](#default-mode) actions, `pods image update`, `logs`, `connect`, and `command` resolve individual containers within a recipe using `<recipe>/<container>` (or `command.container`):

- **Numeric Index:** `example-app/1` selects the first `container` block in the recipe (`1..=N`).
- **Explicit Relative Name:** `example-app/*db` expands directly to `<pod_id>-db` (`example-stack-db`).
- **Unprefixed Name:** `example-app/example-stack-db` or `example-app/db` checks for an exact resolved container name match first (`example-stack-db`), then falls back to `<pod_id>-<name>` (`example-stack-db`).
- **Exclusivity in Lifecycle Actions:** Default mode actions and `pods image update` accept either pod/group targets (`<recipe>`, `@<group>`) or one or more `<recipe>/<container>` container targets in a single invocation, deduplicating repeated `(recipe, container_name)` pairs in first-appearance order. Mixing pod/group targets with `<recipe>/<container>` targets is rejected.
- **Direct Container Name (`connect` only):** If `<target>` contains no `/` and no matching `<target>.pod` file exists in `recipes.path`, `pods connect <target>` connects directly to a running Podman container with that name.

## Default Mode

The default mode executes pod lifecycle operations (`up`, `down`, `create`, `remove`, `start`, `stop`, `restart`, `reload`, `update`) and queries container runtime status (`status`). Omit the mode name to invoke default mode.

```bash
pods [OPTIONS] <action> [TARGETS...]
```

### Actions

| Action                | Description                                                                                                  |
| :-------------------- | :----------------------------------------------------------------------------------------------------------- |
| `create <target...>`  | Create the pod and its containers in deterministic top-to-bottom order without starting them.                |
| `down <target...>`    | Stop all containers in reverse bottom-to-top order and remove the containers and the pod (`stop` + `remove`). |
| `help`                | Display command-line syntax, modes, options, and actions for the default mode.                               |
| `reload <target...>`  | Tear down and rebuild the pod from the `.pod` recipe to apply configuration changes (`down` + `up`).         |
| `remove <target...>`  | Remove all containers in reverse bottom-to-top order and remove the pod without stopping them first.         |
| `restart <target...>` | Stop and start an existing pod and its containers (`stop` + `start`) without removing or recreating them.    |
| `start <target...>`   | Start an existing stopped pod and its containers in top-to-bottom order without recreating them.             |
| `status [target...]`  | Display a formatted status table of managed containers and report invalid recipes with `Recipe Error`.       |
| `stop <target...>`    | Stop a running pod and its containers in reverse bottom-to-top order without removing them.                  |
| `up <target...>`      | Create and start the pod and its containers in deterministic top-to-bottom order (`create` + `start`).       |
| `update <target...>`  | Pull the latest container images and rebuild the pod (`image update` + `reload`).                            |
| `version`             | Display the application version with Git build metadata and exit.                                            |

### Options

| Option            | Description                                                                             |
| :---------------- | :-------------------------------------------------------------------------------------- |
| `--all`           | Include unmanaged Podman containers in `status` output alongside managed containers.    |
| `--config=<name>` | Load an alternative configuration file by name or path.                                 |
| `--debug`         | Enable verbose debug logging.                                                           |
| `--full`          | Insert `TARGET NAMES` (`<recipe>/<index>` for managed containers, `-` for unmanaged) after `NAMES` and append `COMMAND` and `PORTS` columns to `status`. |
| `--simulate`      | Print generated Podman commands without executing them.                                 |

### Usage

```bash
# Display general help or version information (or run 'pods' without arguments)
pods help
pods version

# Create and start a pod and its containers from a single recipe
pods up example-app

# Preview the Podman commands for creating and starting a group of pods without executing them
pods up @backend --simulate

# Stop, start, or restart an existing pod without removing or recreating it
pods stop example-app
pods start example-app
pods restart example-app

# Stop or restart specific containers across one or more recipes without stopping their parent pods
pods stop example-app/db example-app/2
pods restart example-app/*app

# Create a pod and its containers without starting them, or remove a stopped pod
pods create example-app
pods remove example-app

# Tear down and rebuild a pod from its .pod recipe to apply configuration changes
pods reload example-app

# Pull the latest images for a group and rebuild its pods
pods update @backend

# Stop all containers in reverse order and remove multiple recipes and groups
pods down example-app @backend

# Show runtime status for all configured recipes
pods status

# Show extended status (including TARGET NAMES, COMMAND, and PORTS) for a recipe or container
pods status --full example-app
pods status --full example-app/1

# Show status for all containers on the host, including unmanaged containers
pods status --all

# Run lifecycle actions using an alternative configuration and debug logging
pods --config=staging --debug reload example-app
```

## Logs Mode (`log`)

The `logs` (`log`) mode fetches or streams container logs for a recipe's pod or a single container within that pod via `podman pod logs`.

```bash
pods logs [OPTIONS] [ACTION] <recipe>[/<container>]
```

### Actions

| Action   | Description                                                                         |
| :------- | :---------------------------------------------------------------------------------- |
| `follow` | Stream log output continuously until interrupted (`-f`).                            |
| `help`   | Display command-line help for `logs` mode (default when called without arguments).  |
| `show`   | Output logs and exit immediately (default when `<recipe>` is given without action). |

### Options

| Option            | Description                                                                      |
| :---------------- | :------------------------------------------------------------------------------- |
| `--config=<name>` | Load an alternative configuration file by name or path.                          |
| `--debug`         | Enable verbose debug logging.                                                    |
| `--simulate`      | Print the generated `podman pod logs` command without executing it.              |
| `--since=<time>`  | Show logs generated since the given duration (such as `15m`) or timestamp.       |
| `--tail=<n>`      | Show only the last `<n>` lines of log output.                                    |
| `--timestamps`    | Prepend timestamps to each log line.                                             |
| `--until=<time>`  | Show logs generated before the given duration or timestamp.                      |

### Usage

```bash
# Display logs mode help
pods logs help

# Show logs for all containers in a recipe (implicitly uses 'show')
pods logs example-app
pods logs show example-app

# Follow live logs for a specific container by relative name (or via the 'log' alias)
pods logs follow example-app/db
pods log follow example-app/db

# Show the last 100 log lines with timestamps for the first container
pods logs --tail=100 --timestamps show example-app/1

# Filter logs to a specific time window
pods logs --since=1h --until=10m example-app

# Preview the generated Podman logs command using an alternative config
pods --config=staging --debug logs follow example-app/app --simulate
```

## Connect Mode (`con`)

The `connect` (`con`) mode opens an interactive shell (`bash` if available, falling back to `sh`) inside a running container via `podman exec -it`.

```bash
pods connect [OPTIONS] [ACTION] <target>
```

### Actions

| Action  | Description                                                                              |
| :------ | :--------------------------------------------------------------------------------------- |
| `help`  | Display command-line help for `connect` mode (default when called without arguments).    |
| `shell` | Open an interactive shell inside the target container (default when action is omitted).  |

### Options

| Option            | Description                                                            |
| :---------------- | :--------------------------------------------------------------------- |
| `--config=<name>` | Load an alternative configuration file by name or path.                |
| `--debug`         | Enable verbose debug logging.                                          |
| `--simulate`      | Print the generated `podman exec` command without executing it.        |

### Usage

```bash
# Display connect mode help
pods connect help

# Connect to a single-container recipe (implicitly uses 'shell')
pods connect db-server
pods connect shell db-server

# Connect to a specific container in a multi-container recipe by relative name or index (or via 'con')
pods connect example-app/db
pods connect example-app/2
pods con example-app/db

# Connect directly to an unmanaged Podman container by its exact container name
pods connect custom-container-name

# Preview the connection command with debug output and an alternative config
pods --config=staging --debug connect example-app/db --simulate
```

## Command Mode (`cmd`)

The `command` (`cmd`) mode lists or executes maintenance commands declared in `command <name>` blocks inside a recipe's `pod` block via `podman exec -it`.

```bash
pods command [OPTIONS] [ACTION] <recipe> [<command>|<index>]
```

### Actions

| Action | Description                                                                                           |
| :----- | :---------------------------------------------------------------------------------------------------- |
| `exec` | Execute a command by name or 1-based index (default when `<command>` or `<index>` is provided).       |
| `help` | Display command-line help for `command` mode (default when called without arguments).                 |
| `list` | List all valid commands sorted alphabetically with their index (default when `<command>` is omitted). |

### Options

| Option            | Description                                                            |
| :---------------- | :--------------------------------------------------------------------- |
| `--config=<name>` | Load an alternative configuration file by name or path.                |
| `--debug`         | Enable verbose debug logging.                                          |
| `--simulate`      | Print the generated `podman exec` command without executing it.        |

### Usage

```bash
# Display command mode help
pods command help

# List all maintenance commands defined in a recipe
pods command list example-app
pods command example-app

# Execute a maintenance command by its identifier (or via the 'cmd' alias)
pods command exec example-app migrate
pods cmd exec example-app migrate
pods command example-app migrate

# Execute a maintenance command by its 1-based list index
pods command exec example-app 1

# Preview the generated podman exec command using an alternative config
pods --config=staging --debug command exec example-app migrate --simulate
```

## Recipe Mode (`rec`)

The `recipe` (`rec`) mode lists configured recipes with validation status, validates a single `.pod` recipe file, prints recipe file contents with line numbers, or opens a recipe file in the configured editor.

```bash
pods recipe [OPTIONS] [ACTION] [<recipe>]
```

### Actions

| Action    | Description                                                                                                                   |
| :-------- | :---------------------------------------------------------------------------------------------------------------------------- |
| `edit`    | Open the specified `.pod` recipe file in the external editor configured in `pods.conf`.                                       |
| `help`    | Display command-line help for `recipe` mode.                                                                                  |
| `inspect` | Validate the specified `.pod` recipe file and display detailed syntax or semantic diagnostics on error.                       |
| `list`    | List all recipes registered in `pods.conf` with descriptions and validation status (`[OK]`, `[ERROR]`, `[NOT FOUND]`) (default). |
| `show`    | Print the `.pod` recipe file with 1-based line numbers (default when `<recipe>` is given without action).                     |

### Options

| Option            | Description                                                                             |
| :---------------- | :-------------------------------------------------------------------------------------- |
| `--all`           | Include unreferenced `.pod` files found in `recipes.path` when running `recipe list`.   |
| `--config=<name>` | Load an alternative configuration file by name or path.                                 |
| `--debug`         | Enable verbose debug logging.                                                           |
| `--simulate`      | Print the editor command without executing it when running `recipe edit`.               |

### Usage

```bash
# Display recipe mode help
pods recipe help

# List all configured recipes and their validation status (or via the 'rec' alias)
pods recipe
pods recipe list
pods rec list

# List configured recipes plus unreferenced .pod files in the recipes directory
pods recipe list --all

# Validate a single recipe file and inspect syntax or semantic errors
pods recipe inspect example-app
pods rec inspect example-app

# Print a recipe file with line numbers
pods recipe show example-app
pods recipe example-app

# Open a recipe file in the configured text editor
pods recipe edit example-app

# Preview the editor command for a recipe in an alternative config
pods --config=staging --debug recipe edit example-app --simulate
```

## Config Mode (`cfg`)

The `config` (`cfg`) mode validates and inspects the active `pods.conf` configuration (including directory status, group hierarchy trees with recipe validation status, and missing, invalid, or unreferenced recipe warnings), prints the raw configuration file with line numbers, or opens the configuration file in the configured editor.

```bash
pods config [OPTIONS] [ACTION]
```

### Actions

| Action    | Description                                                                                                                |
| :-------- | :------------------------------------------------------------------------------------------------------------------------- |
| `edit`    | Open the active configuration file in the external editor defined by `general.editor`.                                     |
| `help`    | Display command-line help for `config` mode.                                                                               |
| `inspect` | Display the validated configuration tree, recipe inspection status, and validation summary (default when action is omitted). |
| `show`    | Print the active `pods.conf` file with 1-based line numbers.                                                               |

### Options

| Option            | Description                                                               |
| :---------------- | :------------------------------------------------------------------------ |
| `--config=<name>` | Target an alternative configuration file by name or path.                 |
| `--debug`         | Enable verbose debug logging.                                             |
| `--simulate`      | Print the editor command without executing it when running `config edit`. |

### Usage

```bash
# Display config mode help
pods config help

# Inspect the active configuration diagnostic tree and validation summary (or via the 'cfg' alias)
pods config
pods config inspect
pods cfg inspect

# Print the active pods.conf file with 1-based line numbers
pods config show
pods cfg show

# Open the active configuration file in the configured editor
pods config edit

# Inspect or simulate editing an alternative configuration file
pods --config=staging config inspect
pods --config=staging --debug config edit --simulate
```

## Image Mode (`img`)

The `image` (`img`) mode downloads container images defined in target recipes (`update`) or previews and removes unused container images on the host (`prune`). Podman never removes images attached to existing (running or stopped) containers.

```bash
pods image [OPTIONS] [ACTION] [TARGETS...]
```

### Actions

| Action               | Description                                                                                                                       |
| :------------------- | :-------------------------------------------------------------------------------------------------------------------------------- |
| `help`               | Display command-line help for `image` mode (default when called without arguments).                                               |
| `prune`              | Remove unused dangling container images (or all unused images when `--all` is passed).                                            |
| `update <target...>` | Download and pull the latest container images defined in the target recipes (`<recipe>`, `@<group>`, or `<recipe>/<container>`) without executing `reload`. |

### Options

| Option            | Description                                                                                      |
| :---------------- | :----------------------------------------------------------------------------------------------- |
| `--all`           | Target all unused images (including tagged images not used by any container), not just dangling. |
| `--config=<name>` | Load an alternative configuration file by name or path.                                          |
| `--debug`         | Enable verbose debug logging.                                                                    |
| `--force`         | Skip the interactive confirmation prompt before pruning images.                                  |
| `--preview`       | List unused images that would be removed and calculate total reclaimable disk space.             |
| `--simulate`      | Print the generated `podman pull` or `podman image prune` commands without executing them.       |

### Usage

```bash
# Display image mode help
pods image help

# Pull the latest container images for a recipe, group, or specific container without reloading running pods (or via 'img')
pods image update example-app
pods image update example-app/db
pods image update @backend
pods img update @backend

# Preview the generated podman pull commands for a group using an alternative config
pods --config=staging --debug image update @backend --simulate

# Preview dangling images and reclaimable disk space without deleting anything
pods image prune --preview

# Preview all unused images (both dangling and unreferenced tagged images)
pods image prune --all --preview

# Prune dangling images with interactive confirmation
pods image prune

# Prune all unused images without confirmation
pods image prune --all --force

# Preview the generated podman image prune command
pods --debug image prune --all --simulate
```

## Init Mode

The `init` mode initializes a workspace in the current working directory by creating `recipe.pod` and `pods.conf`. If a target file already exists, `pods init` logs a warning and skips overwriting it; if a directory with the same name exists, `pods init` aborts with an error.

```bash
pods init [OPTIONS] [ACTION]
```

### Actions

| Action   | Description                                                                             |
| :------- | :-------------------------------------------------------------------------------------- |
| `create` | Create `./recipe.pod` and `./pods.conf` (default when called without an action).        |
| `help`   | Display command-line help for `init` mode.                                              |

### Options

| Option            | Description                                                                             |
| :---------------- | :-------------------------------------------------------------------------------------- |
| `--debug`         | Enable verbose debug logging.                                                           |
| `--simulate`      | Preview template file creation without writing files to disk.                           |

### Usage

```bash
# Generate default pods.conf and recipe.pod in the current directory
pods init
pods init create

# Preview workspace initialization without writing files
pods init --simulate

# Generate default pods.conf and recipe.pod with debug output enabled
pods --debug init

# Display init mode help
pods init help
```
