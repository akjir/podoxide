_pods_completions() {
    local cur prev
    COMPREPLY=()
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"

    local modes="logs connect init command config recipe image"
    local default_actions="create down help reload remove restart start status stop up update"
    local mode=""
    local action=""

    # Complete arguments for flags that require a value
    case "$prev" in
        --config|-c)
            readarray -t COMPREPLY < <(compgen -f -- "${cur}")
            return 0
            ;;
        --tail|--since|--until)
            return 0
            ;;
    esac

    # Scan previous words to detect the active mode and action
    for (( i=1; i<COMP_CWORD; i++ )); do
        local word="${COMP_WORDS[i]}"

        # Skip flags and their corresponding value arguments
        case "$word" in
            --config|-c|--tail|--since|--until)
                (( i++ ))
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
                create|down|help|reload|remove|restart|start|status|stop|up|update)
                    mode="default"
                    action="$word"
                    ;;
            esac
        elif [[ -z "$action" ]]; then
            action="$word"
        fi
    done

    # Complete global and mode-specific flags
    if [[ "$cur" == -* ]]; then
        local opts="--debug"
        case "$mode" in
            init)
                ;;
            default)
                opts+=" --config --simulate --all --full"
                ;;
            logs)
                opts+=" --config --simulate --tail --since --until --timestamps"
                ;;
            image)
                opts+=" --config --simulate --all --force --preview"
                ;;
            recipe)
                opts+=" --config --simulate --all"
                ;;
            *)
                opts+=" --config --simulate"
                ;;
        esac
        readarray -t COMPREPLY < <(compgen -W "${opts}" -- "${cur}")
        return 0
    fi

    # Complete top-level modes and default lifecycle actions
    if [[ -z "$mode" ]]; then
        readarray -t COMPREPLY < <(compgen -W "${modes} ${default_actions}" -- "${cur}")
        return 0
    fi

    # Complete mode-specific sub-actions
    if [[ -z "$action" ]]; then
        local sub_actions=""
        case "$mode" in
            connect) sub_actions="shell help" ;;
            logs)    sub_actions="show follow help" ;;
            init)    sub_actions="create help" ;;
            command) sub_actions="exec list help" ;;
            config)  sub_actions="show edit help" ;;
            recipe)  sub_actions="edit help list show" ;;
            image)   sub_actions="help prune update" ;;
        esac

        if [[ -n "$sub_actions" ]]; then
            readarray -t COMPREPLY < <(compgen -W "${sub_actions}" -- "${cur}")
        fi
    fi

    return 0
}

complete -F _pods_completions pods ./pods
