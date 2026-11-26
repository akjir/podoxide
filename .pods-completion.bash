# Bash completion for Podoxide (pods)

_pods_comp_filter_and_sort() {
    local prefix="$1"
    shift
    local -a matches=()
    local -A seen=()
    local item
    for item in "$@"; do
        [[ -z "$item" ]] && continue
        if [[ "$item" == "$prefix"* && -z "${seen["$item"]+x}" ]]; then
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
            jdx=$(( jdx - 1 ))
        done
        matches[jdx + 1]="$key"
    done

    COMPREPLY=("${matches[@]}")
}

_pods_comp_contains() {
    local needle="$1"
    shift
    local item
    for item in "$@"; do
        [[ "$item" == "$needle" ]] && return 0
    done
    return 1
}

_pods_comp_complete_config_files() {
    local prefix="$1"
    local opt_prefix="$2"
    local -a raw_entries=()
    local -a candidates=()
    local display_dir="" base_part="$prefix" search_dir="."
    local entry full_path display_entry

    if [[ -z "$opt_prefix" ]] && type compopt >/dev/null 2>&1; then
        compopt -o filenames 2>/dev/null || true
    fi

    if [[ "$prefix" == */* ]]; then
        display_dir="${prefix%/*}/"
        base_part="${prefix##*/}"
    fi
    if [[ -n "$display_dir" ]]; then
        if [[ "$display_dir" == "~/"* && -n "${HOME:-}" ]]; then
            search_dir="${HOME%/}/${display_dir#"~/"}"
        else
            search_dir="$display_dir"
        fi
    fi

    if [[ -d "$search_dir" ]]; then
        readarray -d '' -t raw_entries < <(
            set +f
            shopt -s nullglob
            shopt -u failglob nocaseglob
            _dir="${search_dir%/}"
            if [[ "$base_part" == .* ]]; then
                for _item in "${_dir}"/.*; do
                    _name="${_item##*/}"
                    if [[ "$_name" == "." || "$_name" == ".." ]]; then
                        continue
                    fi
                    if [[ "$_name" == "$base_part"* ]]; then
                        printf '%s\0' "$_name"
                    fi
                done
            else
                for _item in "${_dir}"/*; do
                    _name="${_item##*/}"
                    if [[ "$_name" == "$base_part"* ]]; then
                        printf '%s\0' "$_name"
                    fi
                done
            fi
        )
    fi

    for entry in "${raw_entries[@]}"; do
        full_path="${search_dir%/}/${entry}"
        display_entry="${display_dir}${entry}"
        if [[ -d "$full_path" ]]; then
            if [[ -n "$opt_prefix" ]]; then
                candidates+=("${opt_prefix}${display_entry}/")
            else
                candidates+=("${opt_prefix}${display_entry}")
            fi
        elif [[ -f "$full_path" && "$entry" == *.conf ]]; then
            candidates+=("${opt_prefix}${display_entry}")
        fi
    done

    _pods_comp_filter_and_sort "${opt_prefix}${prefix}" "${candidates[@]}"
    if [[ -n "$opt_prefix" && ${#COMPREPLY[@]} -eq 1 && "${COMPREPLY[0]}" == */ ]] && type compopt >/dev/null 2>&1; then
        compopt -o nospace 2>/dev/null || true
    fi
    return 0
}

_pods_comp_unquote_shell_word() {
    local raw="$1"
    if [[ "$raw" != *[\'\"\\]* ]]; then
        _pods_comp_out="$raw"
        return 0
    fi

    local len="${#raw}"
    local idx ch next
    local in_sq=0 in_dq=0
    local result=""

    for (( idx=0; idx<len; idx++ )); do
        ch="${raw:idx:1}"
        if (( in_sq )); then
            if [[ "$ch" == "'" ]]; then
                in_sq=0
            else
                result+="$ch"
            fi
            continue
        fi
        if (( in_dq )); then
            if [[ "$ch" == '"' ]]; then
                in_dq=0
            elif [[ "$ch" == "\\" ]] && (( idx + 1 < len )); then
                next="${raw:idx+1:1}"
                case "$next" in
                    '"'|"\\"|'$'|'`')
                        result+="$next"
                        idx=$(( idx + 1 ))
                        ;;
                    *)
                        result+="\\"
                        ;;
                esac
            else
                result+="$ch"
            fi
            continue
        fi
        if [[ "$ch" == "'" ]]; then
            in_sq=1
        elif [[ "$ch" == '"' ]]; then
            in_dq=1
        elif [[ "$ch" == "\\" ]] && (( idx + 1 < len )); then
            idx=$(( idx + 1 ))
            result+="${raw:idx:1}"
        else
            result+="$ch"
        fi
    done

    _pods_comp_out="$result"
}

_pods_comp_normalize_name() {
    local value="$1"
    value="${value#"${value%%[![:space:]]*}"}"
    value="${value%"${value##*[![:space:]]}"}"
    value="${value,,}"
    while [[ "$value" =~ [[:space:]]+ ]]; do
        value="${value/"${BASH_REMATCH[0]}"/_}"
    done
    _pods_comp_out="$value"
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
                delta=$(( delta + 1 ))
            elif [[ "$ch" == "}" ]]; then
                delta=$(( delta - 1 ))
            fi
        fi
        result+="$ch"
    done

    _pods_comp_out="$result"
    _pods_comp_delta="$delta"
}

_pods_comp_tokenize_psl_line() {
    local line="$1"
    _pods_comp_tokens=()
    _pods_comp_first_quoted=0
    [[ "$line" == [\"\']* ]] && _pods_comp_first_quoted=1

    local len="${#line}"
    local idx ch next
    local in_sq=0 in_dq=0 has_token=0
    local current=""

    for (( idx=0; idx<len; idx++ )); do
        ch="${line:idx:1}"
        if (( in_sq )); then
            if [[ "$ch" == "'" ]]; then
                in_sq=0
            else
                current+="$ch"
            fi
            continue
        fi
        if (( in_dq )); then
            if [[ "$ch" == '"' ]]; then
                in_dq=0
            elif [[ "$ch" == "\\" ]] && (( idx + 1 < len )); then
                idx=$(( idx + 1 ))
                next="${line:idx:1}"
                case "$next" in
                    '"'|"\\") current+="$next" ;;
                    n) current+=$'\n' ;;
                    r) current+=$'\r' ;;
                    t) current+=$'\t' ;;
                    *) current+="\\$next" ;;
                esac
            else
                current+="$ch"
            fi
            continue
        fi
        case "$ch" in
            " "|$'\t')
                if (( has_token )); then
                    _pods_comp_tokens+=("$current")
                    current=""
                    has_token=0
                fi
                ;;
            "{"|"}")
                if (( has_token )); then
                    _pods_comp_tokens+=("$current")
                    current=""
                    has_token=0
                fi
                _pods_comp_tokens+=("$ch")
                ;;
            "'")
                in_sq=1
                has_token=1
                ;;
            '"')
                in_dq=1
                has_token=1
                ;;
            *)
                current+="$ch"
                has_token=1
                ;;
        esac
    done

    if (( has_token )); then
        _pods_comp_tokens+=("$current")
    fi
}

_pods_comp_is_safe_path() {
    local candidate="$1"
    [[ -n "$candidate" && "$candidate" != -* && "$candidate" != +* && ! "$candidate" =~ (^|/)\.\.(/|$) ]]
}

_pods_comp_resolve_config_path() {
    local raw="$1"
    local was_quoted=0
    _pods_comp_out=""
    if [[ -z "$raw" ]]; then
        if [[ -f "./pods.conf" && -r "./pods.conf" ]]; then
            _pods_comp_out="./pods.conf"
        fi
        return 0
    fi
    if [[ "$raw" == *[\'\"\\]* ]]; then
        was_quoted=1
    fi
    _pods_comp_unquote_shell_word "$raw"
    raw="$_pods_comp_out"
    _pods_comp_out=""
    if (( !was_quoted )) && [[ "$raw" == "~/"* && -n "${HOME:-}" ]]; then
        raw="${HOME%/}/${raw#"~/"}"
    fi
    if [[ -z "$raw" || "$raw" == */ ]] || ! _pods_comp_is_safe_path "$raw"; then
        return 0
    fi
    local resolved="$raw"
    if [[ "$resolved" != *.conf ]]; then
        resolved+=".conf"
    fi
    if [[ "$resolved" != /* && "$resolved" != ./* ]]; then
        resolved="./$resolved"
    fi
    if [[ -f "$resolved" && -r "$resolved" ]]; then
        _pods_comp_out="$resolved"
    fi
    return 0
}

_pods_comp_parse_config() {
    local config_file="$1"
    _pods_comp_recipes_path="."
    _pods_comp_groups=()
    _pods_comp_recipes=()
    [[ -z "$config_file" || ! -f "$config_file" || ! -r "$config_file" ]] && return 0

    local line trimmed token group_name
    local -a _pods_comp_tokens=()
    local depth=0 in_recipes=0 recipes_depth=0 in_groups=0 groups_depth=0
    local _pods_comp_out="" _pods_comp_delta=0 _pods_comp_first_quoted=0
    local count=0 idx

    while IFS= read -r line || [[ -n "$line" ]]; do
        _pods_comp_scan_line "$line"
        trimmed="$_pods_comp_out"
        trimmed="${trimmed#"${trimmed%%[![:space:]]*}"}"
        trimmed="${trimmed%"${trimmed##*[![:space:]]}"}"
        [[ -z "$trimmed" ]] && continue

        _pods_comp_tokenize_psl_line "$trimmed"
        count="${#_pods_comp_tokens[@]}"

        if (( ! _pods_comp_first_quoted && count >= 1 )); then
            if (( depth == 0 && count == 2 )) && [[ "${_pods_comp_tokens[0]}" == "recipes" && "${_pods_comp_tokens[1]}" == "{" ]]; then
                in_recipes=1
                recipes_depth=1
            elif (( in_recipes && !in_groups && depth == recipes_depth && count == 2 )) && [[ "${_pods_comp_tokens[0]}" == "groups" && "${_pods_comp_tokens[1]}" == "{" ]]; then
                in_groups=1
                groups_depth=$(( depth + 1 ))
            elif (( in_recipes && !in_groups && depth == recipes_depth && _pods_comp_delta == 0 && count == 2 )) && [[ "${_pods_comp_tokens[0]}" == "path" ]]; then
                if _pods_comp_is_safe_path "${_pods_comp_tokens[1]}"; then
                    _pods_comp_recipes_path="${_pods_comp_tokens[1]}"
                else
                    _pods_comp_recipes_path=""
                fi
            elif (( in_groups && depth == groups_depth && _pods_comp_delta == 0 )) && [[ "${_pods_comp_tokens[0]}" != "{" && "${_pods_comp_tokens[0]}" != "}" ]]; then
                group_name="${_pods_comp_tokens[0]}"
                if [[ "$group_name" =~ ^[A-Za-z_][A-Za-z0-9_-]*$ ]]; then
                    _pods_comp_groups+=("@${group_name}")
                    for (( idx=1; idx<count; idx++ )); do
                        token="${_pods_comp_tokens[idx]}"
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
    return 0
}

_pods_comp_resolve_recipe_file() {
    local recipes_path="$1"
    local recipe_name="$2"
    _pods_comp_out=""
    if [[ ! "$recipe_name" =~ ^[A-Za-z0-9_][A-Za-z0-9_-]*$ ]] || ! _pods_comp_is_safe_path "$recipes_path"; then
        return 0
    fi
    local candidate="${recipes_path%/}/${recipe_name}.pod"
    if [[ -f "$candidate" && -r "$candidate" ]]; then
        _pods_comp_out="$candidate"
    fi
    return 0
}

_pods_comp_parse_recipe_containers() {
    local recipe_file="$1"
    local recipe_name="$2"
    local container_prefix="$3"
    _pods_comp_containers=()
    [[ -z "$recipe_file" || ! -f "$recipe_file" || ! -r "$recipe_file" ]] && return 0

    local line trimmed header_arg suffix=""
    local -a _pods_comp_tokens=()
    local depth=0 container_index=0 count=0 is_container=0
    local _pods_comp_out="" _pods_comp_delta=0 _pods_comp_first_quoted=0

    while IFS= read -r line || [[ -n "$line" ]]; do
        _pods_comp_scan_line "$line"
        trimmed="$_pods_comp_out"
        trimmed="${trimmed#"${trimmed%%[![:space:]]*}"}"
        trimmed="${trimmed%"${trimmed##*[![:space:]]}"}"
        [[ -z "$trimmed" ]] && continue

        _pods_comp_tokenize_psl_line "$trimmed"
        count="${#_pods_comp_tokens[@]}"
        is_container=0
        header_arg=""

        if (( depth == 0 && ! _pods_comp_first_quoted && count >= 2 )) && [[ "${_pods_comp_tokens[0]}" == "container" ]]; then
            if (( count == 2 )) && [[ "${_pods_comp_tokens[1]}" == "{" ]]; then
                is_container=1
            elif (( count == 3 )) && [[ "${_pods_comp_tokens[2]}" == "{" ]]; then
                is_container=1
                header_arg="${_pods_comp_tokens[1]}"
            elif (( count == 3 )) && [[ "${_pods_comp_tokens[1]}" == "{" && "${_pods_comp_tokens[2]}" == "}" ]]; then
                is_container=1
            elif (( count == 4 )) && [[ "${_pods_comp_tokens[2]}" == "{" && "${_pods_comp_tokens[3]}" == "}" ]]; then
                is_container=1
                header_arg="${_pods_comp_tokens[1]}"
            fi
        fi

        if (( is_container )); then
            container_index=$(( container_index + 1 ))
            _pods_comp_normalize_name "$header_arg"
            header_arg="$_pods_comp_out"
            if [[ -z "$header_arg" ]]; then
                _pods_comp_containers+=("${recipe_name}/${container_index}")
            elif [[ "$header_arg" == \** ]]; then
                suffix="${header_arg#\*}"
                if [[ "$suffix" =~ ^[a-z0-9_][a-z0-9_.-]*$ && "$suffix" != *..* ]]; then
                    if [[ "$container_prefix" == \** ]]; then
                        _pods_comp_containers+=("${recipe_name}/*${suffix}")
                    else
                        _pods_comp_containers+=("${recipe_name}/${suffix}")
                    fi
                fi
            elif [[ "$header_arg" =~ ^[a-z0-9][a-z0-9_.-]*$ && "$header_arg" != *..* ]]; then
                _pods_comp_containers+=("${recipe_name}/${header_arg}")
            fi
        fi

        depth=$(( depth + _pods_comp_delta ))
        (( depth < 0 )) && depth=0
    done < "$recipe_file"
    return 0
}

_pods_comp_parse_recipe_commands() {
    local recipe_file="$1"
    _pods_comp_commands=()
    [[ -z "$recipe_file" || ! -f "$recipe_file" || ! -r "$recipe_file" ]] && return 0

    local line trimmed cmd_name
    local -a _pods_comp_tokens=()
    local depth=0 in_pod=0 count=0
    local _pods_comp_out="" _pods_comp_delta=0 _pods_comp_first_quoted=0

    while IFS= read -r line || [[ -n "$line" ]]; do
        _pods_comp_scan_line "$line"
        trimmed="$_pods_comp_out"
        trimmed="${trimmed#"${trimmed%%[![:space:]]*}"}"
        trimmed="${trimmed%"${trimmed##*[![:space:]]}"}"
        [[ -z "$trimmed" ]] && continue

        _pods_comp_tokenize_psl_line "$trimmed"
        count="${#_pods_comp_tokens[@]}"

        if (( ! _pods_comp_first_quoted )); then
            if (( depth == 0 && count == 3 )) && [[ "${_pods_comp_tokens[0]}" == "pod" && "${_pods_comp_tokens[2]}" == "{" ]]; then
                in_pod=1
            elif (( in_pod && depth == 1 && count == 3 )) && [[ "${_pods_comp_tokens[0]}" == "command" && "${_pods_comp_tokens[2]}" == "{" ]]; then
                cmd_name="${_pods_comp_tokens[1]}"
                if [[ "$cmd_name" =~ ^[A-Za-z_][A-Za-z0-9_-]*$ ]]; then
                    _pods_comp_commands+=("$cmd_name")
                fi
            fi
        fi

        depth=$(( depth + _pods_comp_delta ))
        (( depth < 0 )) && depth=0
        (( in_pod && depth == 0 )) && in_pod=0
    done < "$recipe_file"
    return 0
}

_pods_comp_collect_containers_for_cur() {
    local current_word="$1"
    local recipe_part="${current_word%%/*}"
    local container_part="${current_word#*/}"
    _pods_comp_containers=()
    if _pods_comp_contains "$recipe_part" "${_pods_comp_recipes[@]}"; then
        local _pods_comp_out=""
        _pods_comp_resolve_recipe_file "$_pods_comp_recipes_path" "$recipe_part"
        _pods_comp_parse_recipe_containers "$_pods_comp_out" "$recipe_part" "$container_part"
    fi
}

_pods_comp_collect_lifecycle_targets() {
    local current_word="$1"
    local split_at="$2"
    local has_pod=0 has_container=0
    local item group recipe

    for item in "${targets[@]}"; do
        if [[ "$item" == */* ]]; then
            has_container=1
        else
            has_pod=1
        fi
    done

    if (( has_pod && has_container )); then
        return 0
    fi

    if (( split_at )); then
        (( has_container )) && return 0
        for group in "${_pods_comp_groups[@]}"; do
            if ! _pods_comp_contains "$group" "${targets[@]}"; then
                candidates+=("$group")
            fi
        done
        return 0
    fi

    if [[ "$current_word" == */* ]]; then
        (( has_pod )) && return 0
        _pods_comp_collect_containers_for_cur "$current_word"
        for item in "${_pods_comp_containers[@]}"; do
            if ! _pods_comp_contains "$item" "${targets[@]}"; then
                candidates+=("$item")
            fi
        done
        return 0
    fi

    if (( has_container )); then
        for recipe in "${_pods_comp_recipes[@]}"; do
            candidates+=("${recipe}/")
        done
        return 0
    fi

    for item in "${_pods_comp_groups[@]}" "${_pods_comp_recipes[@]}"; do
        if ! _pods_comp_contains "$item" "${targets[@]}"; then
            candidates+=("$item")
        fi
    done
}

_pods_completions() {
    local _pods_restore_nocasematch=0
    if shopt -q nocasematch; then
        _pods_restore_nocasematch=1
        shopt -u nocasematch
    fi
    _pods_completions_impl "$@"
    local _pods_rc=$?
    if (( _pods_restore_nocasematch )); then
        shopt -s nocasematch
    fi
    return "$_pods_rc"
}

_pods_completions_impl() {
    local cur prev prev2 i word
    COMPREPLY=()
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev=""
    prev2=""
    (( COMP_CWORD >= 1 )) && prev="${COMP_WORDS[COMP_CWORD-1]}"
    (( COMP_CWORD >= 2 )) && prev2="${COMP_WORDS[COMP_CWORD-2]}"

    # Complete arguments for value-taking flags (split '=', 3-word '=', space-separated, and glued)
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
            if [[ "$cur" == "=" ]]; then
                _pods_comp_complete_config_files "" ""
            else
                _pods_comp_complete_config_files "$cur" ""
            fi
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
    local unknown_first=0 unknown_action=0 split_at_prefix=0
    local has_all=0 has_config=0 has_force=0 has_full=0 has_log_opt=0 has_preview=0
    local -a targets=()

    # Scan previous words to detect active mode, action, flags, and positional targets
    for (( i=1; i<COMP_CWORD; i++ )); do
        word="${COMP_WORDS[i]}"
        case "$word" in
            --config)
                has_config=1
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
                has_config=1
                config_arg="${word#--config=}"
                continue
                ;;
            --since|--until)
                has_log_opt=1
                if (( i + 1 < COMP_CWORD )) && [[ "${COMP_WORDS[i+1]}" == "=" ]]; then
                    if (( i + 2 < COMP_CWORD )); then
                        (( i += 2 ))
                    else
                        (( i += 1 ))
                        continue
                    fi
                elif (( i + 1 < COMP_CWORD )); then
                    (( i += 1 ))
                else
                    continue
                fi
                while (( i + 1 < COMP_CWORD )) && [[ "${COMP_WORDS[i+1]}" == ":" ]]; do
                    if (( i + 2 < COMP_CWORD )); then
                        (( i += 2 ))
                    else
                        return 0
                    fi
                done
                if (( i + 1 == COMP_CWORD )) && [[ "$cur" == ":" ]]; then
                    return 0
                fi
                continue
                ;;
            --since=*|--until=*)
                has_log_opt=1
                while (( i + 1 < COMP_CWORD )) && [[ "${COMP_WORDS[i+1]}" == ":" ]]; do
                    if (( i + 2 < COMP_CWORD )); then
                        (( i += 2 ))
                    else
                        return 0
                    fi
                done
                if (( i + 1 == COMP_CWORD )) && [[ "$cur" == ":" ]]; then
                    return 0
                fi
                continue
                ;;
            --tail)
                has_log_opt=1
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
            --tail=*)
                has_log_opt=1
                continue
                ;;
            --timestamps)
                has_log_opt=1
                continue
                ;;
            --all)
                has_all=1
                continue
                ;;
            --force)
                has_force=1
                continue
                ;;
            --full)
                has_full=1
                continue
                ;;
            --preview)
                has_preview=1
                continue
                ;;
            -*)
                continue
                ;;
            "@")
                if (( i + 1 < COMP_CWORD )) && [[ -n "${COMP_WORDS[i+1]}" && "${COMP_WORDS[i+1]}" != -* ]]; then
                    word="@${COMP_WORDS[i+1]}"
                    (( i += 1 ))
                elif (( i + 1 == COMP_CWORD )) && [[ -n "$cur" && "$cur" != -* ]]; then
                    split_at_prefix=1
                    continue
                fi
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
        (( split_at_prefix )) && return 0
        local -a top_candidates=()
        if (( has_full )); then
            (( !has_force && !has_preview && !has_log_opt )) && top_candidates=(status)
        elif (( has_force || has_preview )); then
            (( !has_log_opt )) && top_candidates=(image)
        elif (( has_log_opt )); then
            (( !has_all )) && top_candidates=(logs)
        elif (( has_all )); then
            top_candidates=(image recipe status)
        elif (( has_config )); then
            top_candidates=(command config connect image logs recipe "${default_actions[@]}")
        else
            top_candidates=("${modes[@]}" "${default_actions[@]}")
        fi
        _pods_comp_filter_and_sort "$cur" "${top_candidates[@]}"
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
        (( split_at_prefix )) && return 0
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
            image)
                if (( has_preview || has_force || has_all )); then
                    candidates=(prune)
                else
                    candidates=(help prune update)
                fi
                ;;
            init)
                (( !has_config )) && candidates=(create help)
                ;;
            logs)
                if [[ "$cur" == */* ]]; then
                    _pods_comp_collect_containers_for_cur "$cur"
                    candidates=("${_pods_comp_containers[@]}")
                elif (( has_log_opt )); then
                    candidates=(follow show "${_pods_comp_recipes[@]}")
                else
                    candidates=(follow help show "${_pods_comp_recipes[@]}")
                fi
                ;;
            recipe)
                if (( has_all )); then
                    candidates=(list)
                else
                    candidates=(edit help inspect list show "${_pods_comp_recipes[@]}")
                fi
                ;;
        esac
        _pods_comp_filter_and_sort "$cur" "${candidates[@]}"
        return 0
    fi

    # Complete dynamic targets, containers, and recipe maintenance commands
    case "$mode" in
        default)
            case "$action" in
                help|version) return 0 ;;
                status)
                    (( has_all )) && return 0
                    _pods_comp_collect_lifecycle_targets "$cur" "$split_at_prefix"
                    ;;
                *)
                    _pods_comp_collect_lifecycle_targets "$cur" "$split_at_prefix"
                    ;;
            esac
            ;;
        image)
            if [[ "$action" == "update" ]]; then
                _pods_comp_collect_lifecycle_targets "$cur" "$split_at_prefix"
            fi
            ;;
        command)
            (( split_at_prefix )) && return 0
            if [[ ( "$action" == "list" || "$action" == "exec" ) && ${#targets[@]} -eq 0 ]]; then
                candidates=("${_pods_comp_recipes[@]}")
            elif [[ ( "$action" == "exec" || "$action" == "implicit" ) && ${#targets[@]} -eq 1 ]]; then
                recipe_part="${targets[0]}"
                if _pods_comp_contains "$recipe_part" "${_pods_comp_recipes[@]}"; then
                    _pods_comp_resolve_recipe_file "$_pods_comp_recipes_path" "$recipe_part"
                    _pods_comp_parse_recipe_commands "$_pods_comp_out"
                    candidates=("${_pods_comp_commands[@]}")
                fi
            fi
            ;;
        connect|logs)
            (( split_at_prefix )) && return 0
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
            (( split_at_prefix )) && return 0
            if [[ ( "$action" == "edit" || "$action" == "inspect" || "$action" == "show" ) && ${#targets[@]} -eq 0 ]]; then
                candidates=("${_pods_comp_recipes[@]}")
            fi
            ;;
    esac

    local filter_cur="$cur"
    (( split_at_prefix )) && filter_cur="@${cur}"
    _pods_comp_filter_and_sort "$filter_cur" "${candidates[@]}"
    if (( ${#COMPREPLY[@]} == 1 )) && [[ "${COMPREPLY[0]}" == */ ]] && type compopt >/dev/null 2>&1; then
        compopt -o nospace 2>/dev/null || true
    fi
    return 0
}

complete -F _pods_completions pods ./pods
