Output img/demo_tui_zsh.gif

Set Shell "bash"
Set FontSize 20
Set Width 1480
Set Height 720
Set Framerate 30
Set Padding 24
Set CursorBlink true
Set TypingSpeed 40ms

Require "twatch"

Sleep 1s
Type "ZDOTDIR=/work/docker/zsh twatch -L 5000 -- zsh -i"
Enter
Sleep 3s

Type "i"
Sleep 500ms

Type "pwd"
Enter
Sleep 800ms

Type "printf 'alpha\nbeta\ngamma\n'"
Enter
Sleep 800ms

Type "ls -la"
Enter
Sleep 1500ms

Ctrl+G
Sleep 400ms

Backspace
Sleep 700ms
Up
Sleep 500ms
Up
Sleep 500ms
Up
Sleep 1600ms
Down
Sleep 500ms
Down
Sleep 500ms

Backspace
Sleep 500ms

Type "q"
Sleep 300ms
Type "y"
Sleep 800ms
