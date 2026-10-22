_pods_completions() {
    local cur prev
    COMPREPLY=()
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"

    local modes="logs connect init command config recipe image"
    local default_actions="create help recreate remove status update"
    local mode=""
    local action=""

    for (( i=1; i<COMP_CWORD; i++ )); do
        local word="${COMP_WORDS[i]}"
        if [[ "$word" == -* ]]; then
            continue
        fi

        if [[ -z "$mode" ]]; then
            if [[ " $modes " =~ " $word " ]]; then
                mode="$word"
            elif [[ " $default_actions " =~ " $word " ]]; then
                mode="default"
                action="$word"
            fi
        elif [[ -z "$action" ]]; then
            action="$word"
        fi
    done

    if [[ ${cur} == -* ]]; then
        local opts="--config --debug --simulate"
        case "$mode" in
            default)
                opts+=" --all --full"
                ;;
            logs)
                opts+=" --tail --since --until --timestamps"
                ;;
            image)
                opts+=" --all --force --preview"
                ;;
            recipe)
                opts+=" --all"
                ;;
        esac
        COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
        return 0
    fi

    if [[ -z "$mode" ]]; then
        COMPREPLY=( $(compgen -W "${modes} ${default_actions}" -- "${cur}") )
        return 0
    fi

    if [[ -z "$action" ]]; then
        case "$mode" in
            connect)
                COMPREPLY=( $(compgen -W "shell help" -- "${cur}") )
                ;;
            logs)
                COMPREPLY=( $(compgen -W "show follow help" -- "${cur}") )
                ;;
            init)
                COMPREPLY=( $(compgen -W "help" -- "${cur}") )
                ;;
            command)
                COMPREPLY=( $(compgen -W "exec list help" -- "${cur}") )
                ;;
            config)
                COMPREPLY=( $(compgen -W "show edit help" -- "${cur}") )
                ;;
            recipe)
                COMPREPLY=( $(compgen -W "edit help list show" -- "${cur}") )
                ;;
            image)
                COMPREPLY=( $(compgen -W "prune help" -- "${cur}") )
                ;;
        esac
    fi

    return 0
}

complete -F _pods_completions pods ./pods
