# The prompt the corporate shell is configured with, kept apart from the shell
# settings so that a leaf repository can take one without the other.
typeset -g POWERLEVEL9K_LEFT_PROMPT_ELEMENTS=(dir vcs)
typeset -g POWERLEVEL9K_RIGHT_PROMPT_ELEMENTS=(status)
