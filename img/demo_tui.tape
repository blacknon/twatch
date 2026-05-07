Output img/demo_tui.gif

Set Shell "zsh"
Set FontSize 20
Set Width 1480
Set Height 720
Set Framerate 30
Set Padding 24
Set CursorBlink true
Set TypingSpeed 40ms

Require "./target/debug/twatch"

Sleep 1s
Type "./target/debug/twatch htop"
Enter
Sleep 2s
Type "d"
Sleep 6s

Backspace
Sleep 800ms

Down
Sleep 400ms
Down
Sleep 400ms
Down
Sleep 400ms
Down
Sleep 3s
PageUp
Sleep 3s
Backspace
Sleep 400ms

Type "s"
Sleep 600ms

Type "S"
Sleep 6s

Type "q"
Sleep 400ms
Type "y"
Sleep 800ms
