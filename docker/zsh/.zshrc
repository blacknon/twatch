autoload -Uz colors && colors

setopt prompt_subst
unsetopt beep

PROMPT='%F{green}demo%f:%F{cyan}%~%f %# '
RPROMPT=''

export LANG=C.UTF-8
