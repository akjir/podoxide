# Bash completion for Podoxide (pods)

_pods_comp_filter_and_sort() {
    local prefix="$1"
    shift
    local -a matches=()
    local -A seen=()
    local item
    for item in "$@"; do
        if [[ "$item" == "$prefix"* && -z "${seen[$item]+x}" ]]; then
            seen["$item"]=1
            matches+=("$item")
        fi
    done

    local count="${#matches[@]}"
    local idx jdx key
    local LC_ALL=C
    for (( idx=1; idx<count; idx++ )); do
        key="${matches[idx]}"
        jdx=$(( idx - 1 ))
        while (( jdx >= 0 )) && [[ "${matches[jdx]}" > "$key" ]]; do
            matches[jdx + 1]="${matches[jdx]}"
            (( jdx-- ))
        done
        matches[jdx + 1]="$key"
    done

    COMPREPLY=("${matches[@]}")
}

_pods_comp_complete_config_files() {
    local prefix="$1"
    local opt_prefix="$2"
    local -a raw_entries=()
    local -a candidates=()
    local entry

    if [[ -z "$opt_prefix" ]] && type compopt >/dev/null 2>&1; then
        compopt -o filenames 2>/dev/null || true
    fi

    readarray -t raw_entries < <(compgen -f -- "$prefix")
    for entry in "${raw_entries[@]}"; do
        if [[ -d "$entry" || ( -f "$entry" && "$entry" == *.conf ) ]]; then
            candidates+=("${opt_prefix}${entry}")
        fi
    done

    _pods_comp_filter_and_sort "${opt_prefix}${prefix}" "${candidates[@]}"
}

_pods_comp_scan_line() {
    local line="$1"
    _pods_comp_delta=0
    if [[ "$line" != *#* && "$line" != *[\{\}]* ]]; then
        _pods_comp_out="$line"
        return 0
    fi

    local len="${#line}"
    local idx ch
    local in_sq=0
    local in_dq=0
    local escaped=0
    local delta=0
    local result=""

    for (( idx=0; idx<len; idx++ )); do
        ch="${line:idx:1}"
        if (( escaped )); then
            result+="$ch"
            escaped=0
            continue
        fi
        if (( in_dq )) && [[ "$ch" == "\\" ]]; then
            result+="$ch"
            escaped=1
            continue
        fi
        if (( !in_dq )) && [[ "$ch" == "'" ]]; then
            in_sq=$(( 1 - in_sq ))
            result+="$ch"
            continue
        fi
        if (( !in_sq )) && [[ "$ch" == '"' ]]; then
            in_dq=$(( 1 - in_dq ))
            result+="$ch"
            continue
        fi
        if (( !in_sq && !in_dq )); then
            if [[ "$ch" == "#" ]]; then
                break
            elif [[ "$ch" == "{" ]]; then
                (( delta++ ))
            elif [[ "$ch" == "}" ]]; then
                (( delta-- ))
            fi
        fi
        result+="$ch"
    done

    _pods_comp_out="$result"
    _pods_comp_delta="$delta"
}

_pods_comp_unquote() {
    local value="$1"
    if [[ ${#value} -ge 2 ]]; then
        if [[ "${value:0:1}" == '"' && "${value: -1}" == '"' ]]; then
            value="${value:1:${#value}-2}"
        elif [[ "${value:0:1}" == "'" && "${value: -1}" == "'" ]]; then
            value="${value:1:${#value}-2}"
        fi
    fi
    _pods_comp_out="$value"
}

_pods_comp_is_safe_path() {
    local candidate="$1"
    [[ -n "$candidate" && "$candidate" != -* && "$candidate" != +* && ! "$candidate" =~ (^|/)\.\.(/|$) ]]
}

_pods_comp_resolve_config_path() {
    local raw="$1"
    _pods_comp_out=""
    if [[ -z "$raw" ]]; then
        [[ -f "./pods.conf" && -r "./pods.conf" ]] && _pods_comp_out="./pods.conf"
        return 0
    fi
    if [[ "$raw" == */ ]] || ! _pods_comp_is_safe_path "$raw"; then
        return 0
    fi
    local resolved="$raw"
    [[ "$resolved" != *.conf ]] && resolved+=".conf"
    [[ "$resolved" != /* && "$resolved" != ./* ]] && resolved="./$resolved"
    [[ -f "$resolved" && -r "$resolved" ]] && _pods_comp_out="$resolved"
}

_pods_comp_parse_config() {
    local config_file="$1"
    _pods_comp_recipes_path="."
    _pods_comp_groups=()
    _pods_comp_recipes=()
    [[ -z "$config_file" || ! -f "$config_file" || ! -r "$config_file" ]] && return 0

    local line trimmed token group_name raw_path
    local -a tokens=()
    local depth=0 in_recipes=0 recipes_depth=0 in_groups=0 groups_depth=0
    local _pods_comp_out="" _pods_comp_delta=0 idx

    while IFS= read -r line || [[ -n "$line" ]]; do
        _pods_comp_scan_line "$line"
        trimmed="$_pods_comp_out"
        trimmed="${trimmed#"${trimmed%%[![:space:]]*}"}"
        trimmed="${trimmed%"${trimmed##*[![:space:]]}"}"
        [[ -z "$trimmed" ]] && continue

        if (( depth == 0 )) && [[ "$trimmed" =~ ^recipes[[:space:]]*\{ ]]; then
            in_recipes=1
            recipes_depth=1
        elif (( in_recipes && !in_groups && depth == recipes_depth )) && [[ "$trimmed" =~ ^groups[[:space:]]*\{ ]]; then
            in_groups=1
            groups_depth=$(( depth + 1 ))
        elif (( in_recipes && !in_groups && depth == recipes_depth )) && [[ "$trimmed" =~ ^path[[:space:]]+(.+)$ ]]; then
            raw_path="${BASH_REMATCH[1]}"
            _pods_comp_unquote "$raw_path"
            if _pods_comp_is_safe_path "$_pods_comp_out"; then
                _pods_comp_recipes_path="$_pods_comp_out"
            else
                _pods_comp_recipes_path=""
            fi
        elif (( in_groups && depth == groups_depth )) && [[ "$trimmed" != *[\{\}]* ]]; then
            IFS=$' \t' read -r -a tokens <<< "$trimmed"
            if (( ${#tokens[@]} >= 1 )); then
                group_name="${tokens[0]}"
                if [[ "$group_name" =~ ^[A-Za-z_][A-Za-z0-9_-]*$ ]]; then
                    _pods_comp_groups+=("@${group_name}")
                    for (( idx=1; idx<${#tokens[@]}; idx++ )); do
                        token="${tokens[idx]}"
                        if [[ "$token" != @* && "$token" =~ ^[A-Za-z0-9_][A-Za-z0-9_-]*$ ]]; then
                            _pods_comp_recipes+=("$token")
                        fi
                    done
                fi
            fi
        fi

        depth=$(( depth + _pods_comp_delta ))
        (( depth < 0 )) && depth=0
        (( in_groups && depth < groups_depth )) && in_groups=0
        (( in_recipes && depth < recipes_depth )) && in_recipes=0
    done < "$config_file"
}

_pods_comp_resolve_recipe_file() {
    local recipes_path="$1"
    local recipe_name="$2"
    _pods_comp_out=""
    if [[ ! "$recipe_name" =~ ^[A-Za-z0-9_][A-Za-z0-9_-]*$ ]] || ! _pods_comp_is_safe_path "$recipes_path"; then
        return 0
    fi
    local candidate="${recipes_path%/}/${recipe_name}.pod"
    [[ -f "$candidate" && -r "$candidate" ]] && _pods_comp_out="$candidate"
}

_pods_comp_parse_recipe_containers() {
    local recipe_file="$1"
    local recipe_name="$2"
    local container_prefix="$3"
    _pods_comp_containers=()
    [[ -z "$recipe_file" || ! -f "$recipe_file" || ! -r "$recipe_file" ]] && return 0

    local line trimmed header_arg suffix=""
    local depth=0 container_index=0
    local _pods_comp_out="" _pods_comp_delta=0

    while IFS= read -r line || [[ -n "$line" ]]; do
        _pods_comp_scan_line "$line"
        trimmed="$_pods_comp_out"
        trimmed="${trimmed#"${trimmed%%[![:space:]]*}"}"
        trimmed="${trimmed%"${trimmed##*[![:space:]]}"}"
        [[ -z "$trimmed" ]] && continue

        if (( depth == 0 )) && [[ "$trimmed" =~ ^container([[:space:]]+([^[:space:]\{]+))?[[:space:]]*\{ ]]; then
            (( container_index++ ))
            header_arg="${BASH_REMATCH[2]}"
            _pods_comp_unquote "$header_arg"
            header_arg="${_pods_comp_out,,}"
            if [[ -z "$header_arg" ]]; then
                _pods_comp_containers+=("${recipe_name}/${container_index}")
            elif [[ "$header_arg" == \** ]]; then
                suffix="${header_arg#\*}"
                if [[ "$suffix" =~ ^[a-z0-9_][a-z0-9_.-]*$ ]]; then
                    if [[ "$container_prefix" == \** ]]; then
                        _pods_comp_containers+=("${recipe_name}/*${suffix}")
                    else
                        _pods_comp_containers+=("${recipe_name}/${suffix}")
                    fi
                fi
            elif [[ "$header_arg" =~ ^[a-z0-9][a-z0-9_.-]*$ ]]; then
                _pods_comp_containers+=("${recipe_name}/${header_arg}")
            fi
        fi

        depth=$(( depth + _pods_comp_delta ))
        (( depth < 0 )) && depth=0
    done < "$recipe_file"
}

_pods_comp_parse_recipe_commands() {
    local recipe_file="$1"
    _pods_comp_commands=()
    [[ -z "$recipe_file" || ! -f "$recipe_file" || ! -r "$recipe_file" ]] && return 0

    local line trimmed cmd_name
    local depth=0 in_pod=0
    local _pods_comp_out="" _pods_comp_delta=0

    while IFS= read -r line || [[ -n "$line" ]]; do
        _pods_comp_scan_line "$line"
        trimmed="$_pods_comp_out"
        trimmed="${trimmed#"${trimmed%%[![:space:]]*}"}"
        trimmed="${trimmed%"${trimmed##*[![:space:]]}"}"
        [[ -z "$trimmed" ]] && continue

        if (( depth == 0 )) && [[ "$trimmed" =~ ^pod[[:space:]]+[^[:space:]\{]+[[:space:]]*\{ ]]; then
            in_pod=1
        elif (( in_pod && depth == 1 )) && [[ "$trimmed" =~ ^command[[:space:]]+([^[:space:]\{]+)[[:space:]]*\{ ]]; then
            cmd_name="${BASH_REMATCH[1]}"
            _pods_comp_unquote "$cmd_name"
            cmd_name="$_pods_comp_out"
            if [[ "$cmd_name" =~ ^[A-Za-z_][A-Za-z0-9_-]*$ ]]; then
                _pods_comp_commands+=("$cmd_name")
            fi
        fi

        depth=$(( depth + _pods_comp_delta ))
        (( depth < 0 )) && depth=0
        (( in_pod && depth == 0 )) && in_pod=0
    done < "$recipe_file"
}

_pods_comp_is_registered_recipe() {
    local needle="$1"
    shift
    local item
    for item in "$@"; do
        [[ "$item" == "$needle" ]] && return 0
    done
    return 1
}

_pods_comp_collect_containers_for_cur() {
    local current_word="$1"
    local recipe_part="${current_word%%/*}"
    local container_part="${current_word#*/}"
    _pods_comp_containers=()
    if _pods_comp_is_registered_recipe "$recipe_part" "${_pods_comp_recipes[@]}"; then
        local _pods_comp_out=""
        _pods_comp_resolve_recipe_file "$_pods_comp_recipes_path" "$recipe_part"
        _pods_comp_parse_recipe_containers "$_pods_comp_out" "$recipe_part" "$container_part"
    fi
}

_pods_completions() {
    local cur prev prev2 i word
    COMPREPLY=()
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev=""
    prev2=""
    (( COMP_CWORD >= 1 )) && prev="${COMP_WORDS[COMP_CWORD-1]}"
    (( COMP_CWORD >= 2 )) && prev2="${COMP_WORDS[COMP_CWORD-2]}"

    # Complete arguments for value-taking flags (split '=', space-separated, and glued)
    if [[ "$prev" == "=" ]]; then
        case "$prev2" in
            --config)
                _pods_comp_complete_config_files "$cur" ""
                return 0
                ;;
            --since|--tail|--until)
                return 0
                ;;
        esac
    fi
    case "$prev" in
        --config)
            _pods_comp_complete_config_files "$cur" ""
            return 0
            ;;
        --since|--tail|--until)
            return 0
            ;;
    esac
    if [[ "$cur" == --config=* ]]; then
        _pods_comp_complete_config_files "${cur#--config=}" "--config="
        return 0
    fi
    case "$cur" in
        --since=*|--tail=*|--until=*)
            return 0
            ;;
    esac

    local -a modes=(command config connect image init logs recipe)
    local -a default_actions=(create down help reload remove restart start status stop up update version)
    local mode="" action="" config_arg=""
    local unknown_first=0 unknown_action=0 has_force=0 has_preview=0
    local -a targets=()

    # Scan previous words to detect active mode, action, flags, and positional targets
    for (( i=1; i<COMP_CWORD; i++ )); do
        word="${COMP_WORDS[i]}"
        case "$word" in
            --config)
                if (( i + 1 < COMP_CWORD )) && [[ "${COMP_WORDS[i+1]}" == "=" ]]; then
                    if (( i + 2 < COMP_CWORD )); then
                        config_arg="${COMP_WORDS[i+2]}"
                        (( i += 2 ))
                    else
                        (( i += 1 ))
                    fi
                elif (( i + 1 < COMP_CWORD )); then
                    config_arg="${COMP_WORDS[i+1]}"
                    (( i += 1 ))
                fi
                continue
                ;;
            --config=*)
                config_arg="${word#--config=}"
                continue
                ;;
            --since|--tail|--until)
                if (( i + 1 < COMP_CWORD )) && [[ "${COMP_WORDS[i+1]}" == "=" ]]; then
                    if (( i + 2 < COMP_CWORD )); then
                        (( i += 2 ))
                    else
                        (( i += 1 ))
                    fi
                elif (( i + 1 < COMP_CWORD )); then
                    (( i += 1 ))
                fi
                continue
                ;;
            --since=*|--tail=*|--until=*)
                continue
                ;;
            --force)
                has_force=1
                continue
                ;;
            --preview)
                has_preview=1
                continue
                ;;
            -*)
                continue
                ;;
        esac

        if [[ -z "$mode" ]]; then
            case "$word" in
                command|cmd) mode="command" ;;
                config|cfg)  mode="config" ;;
                connect|con) mode="connect" ;;
                image|img)   mode="image" ;;
                init)        mode="init" ;;
                logs|log)    mode="logs" ;;
                recipe|rec)  mode="recipe" ;;
                create|down|help|reload|remove|restart|start|status|stop|up|update|version)
                    mode="default"
                    action="$word"
                    ;;
                *)
                    unknown_first=1
                    break
                    ;;
            esac
        elif [[ -z "$action" ]]; then
            case "$mode" in
                command)
                    case "$word" in
                        exec|help|list) action="$word" ;;
                        *) action="implicit"; targets+=("$word") ;;
                    esac
                    ;;
                config)
                    case "$word" in
                        edit|help|inspect|show) action="$word" ;;
                        *) unknown_action=1; break ;;
                    esac
                    ;;
                connect)
                    case "$word" in
                        help|shell) action="$word" ;;
                        *) action="shell"; targets+=("$word") ;;
                    esac
                    ;;
                image)
                    case "$word" in
                        help|prune|update) action="$word" ;;
                        *) unknown_action=1; break ;;
                    esac
                    ;;
                init)
                    case "$word" in
                        create|help) action="$word" ;;
                        *) unknown_action=1; break ;;
                    esac
                    ;;
                logs)
                    case "$word" in
                        follow|help|show) action="$word" ;;
                        *) action="show"; targets+=("$word") ;;
                    esac
                    ;;
                recipe)
                    case "$word" in
                        edit|help|inspect|list|show) action="$word" ;;
                        *) action="show"; targets+=("$word") ;;
                    esac
                    ;;
            esac
        else
            targets+=("$word")
        fi
    done

    (( unknown_first || unknown_action )) && return 0

    # Complete action-scoped flags
    if [[ "$cur" == -* ]]; then
        local -a opts=()
        case "$mode" in
            "")
                opts=(--config --debug --simulate)
                ;;
            default)
                case "$action" in
                    help|version) opts=(--config --debug) ;;
                    status)
                        if (( ${#targets[@]} == 0 )); then
                            opts=(--all --config --debug --full --simulate)
                        else
                            opts=(--config --debug --full --simulate)
                        fi
                        ;;
                    *) opts=(--config --debug --simulate) ;;
                esac
                ;;
            image)
                case "$action" in
                    help)   opts=(--config --debug) ;;
                    update) opts=(--config --debug --simulate) ;;
                    *)
                        opts=(--all --config --debug)
                        (( !has_preview )) && opts+=(--force)
                        (( !has_force )) && opts+=(--preview)
                        opts+=(--simulate)
                        ;;
                esac
                ;;
            init)
                if [[ "$action" == "help" ]]; then
                    opts=(--debug)
                else
                    opts=(--debug --simulate)
                fi
                ;;
            logs)
                if [[ "$action" == "help" ]]; then
                    opts=(--config --debug)
                else
                    opts=(--config --debug --simulate --since --tail --timestamps --until)
                fi
                ;;
            recipe)
                case "$action" in
                    help)    opts=(--config --debug) ;;
                    ""|list) opts=(--all --config --debug --simulate) ;;
                    *)       opts=(--config --debug --simulate) ;;
                esac
                ;;
            *)
                if [[ "$action" == "help" ]]; then
                    opts=(--config --debug)
                else
                    opts=(--config --debug --simulate)
                fi
                ;;
        esac
        _pods_comp_filter_and_sort "$cur" "${opts[@]}"
        return 0
    fi

    # Complete top-level modes and default lifecycle actions
    if [[ -z "$mode" ]]; then
        _pods_comp_filter_and_sort "$cur" "${modes[@]}" "${default_actions[@]}"
        return 0
    fi

    # Resolve active config for dynamic target, container, and command completions
    local _pods_comp_out="" _pods_comp_recipes_path="."
    local -a _pods_comp_groups=() _pods_comp_recipes=()
    local -a _pods_comp_containers=() _pods_comp_commands=()
    local -a candidates=()
    local recipe_part=""

    if [[ "$mode" != "init" && "$mode" != "config" && "$action" != "help" && "$action" != "version" ]]; then
        _pods_comp_resolve_config_path "$config_arg"
        _pods_comp_parse_config "$_pods_comp_out"
    fi

    # Complete mode-specific sub-actions (plus implicit targets where supported)
    if [[ -z "$action" ]]; then
        case "$mode" in
            command) candidates=(exec help list "${_pods_comp_recipes[@]}") ;;
            config)  candidates=(edit help inspect show) ;;
            connect)
                if [[ "$cur" == */* ]]; then
                    _pods_comp_collect_containers_for_cur "$cur"
                    candidates=("${_pods_comp_containers[@]}")
                else
                    candidates=(help shell "${_pods_comp_recipes[@]}")
                fi
                ;;
            image)   candidates=(help prune update) ;;
            init)    candidates=(create help) ;;
            logs)
                if [[ "$cur" == */* ]]; then
                    _pods_comp_collect_containers_for_cur "$cur"
                    candidates=("${_pods_comp_containers[@]}")
                else
                    candidates=(follow help show "${_pods_comp_recipes[@]}")
                fi
                ;;
            recipe)  candidates=(edit help inspect list show "${_pods_comp_recipes[@]}") ;;
        esac
        _pods_comp_filter_and_sort "$cur" "${candidates[@]}"
        return 0
    fi

    # Complete dynamic targets, containers, and recipe maintenance commands
    case "$mode" in
        default)
            case "$action" in
                help|version) return 0 ;;
                *)
                    if [[ "$cur" == */* ]]; then
                        _pods_comp_collect_containers_for_cur "$cur"
                        candidates=("${_pods_comp_containers[@]}")
                    else
                        candidates=("${_pods_comp_groups[@]}" "${_pods_comp_recipes[@]}")
                    fi
                    ;;
            esac
            ;;
        image)
            if [[ "$action" == "update" ]]; then
                if [[ "$cur" == */* ]]; then
                    _pods_comp_collect_containers_for_cur "$cur"
                    candidates=("${_pods_comp_containers[@]}")
                else
                    candidates=("${_pods_comp_groups[@]}" "${_pods_comp_recipes[@]}")
                fi
            fi
            ;;
        command)
            if [[ ( "$action" == "list" || "$action" == "exec" ) && ${#targets[@]} -eq 0 ]]; then
                candidates=("${_pods_comp_recipes[@]}")
            elif [[ ( "$action" == "exec" || "$action" == "implicit" ) && ${#targets[@]} -eq 1 ]]; then
                recipe_part="${targets[0]}"
                if _pods_comp_is_registered_recipe "$recipe_part" "${_pods_comp_recipes[@]}"; then
                    _pods_comp_resolve_recipe_file "$_pods_comp_recipes_path" "$recipe_part"
                    _pods_comp_parse_recipe_commands "$_pods_comp_out"
                    candidates=("${_pods_comp_commands[@]}")
                fi
            fi
            ;;
        connect|logs)
            if [[ "$action" != "help" && ${#targets[@]} -eq 0 ]]; then
                if [[ "$cur" == */* ]]; then
                    _pods_comp_collect_containers_for_cur "$cur"
                    candidates=("${_pods_comp_containers[@]}")
                else
                    candidates=("${_pods_comp_recipes[@]}")
                fi
            fi
            ;;
        recipe)
            if [[ ( "$action" == "edit" || "$action" == "inspect" || "$action" == "show" ) && ${#targets[@]} -eq 0 ]]; then
                candidates=("${_pods_comp_recipes[@]}")
            fi
            ;;
    esac

    _pods_comp_filter_and_sort "$cur" "${candidates[@]}"
    return 0
}

complete -F _pods_completions pods ./pods
