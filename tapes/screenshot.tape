Output /tmp/twatch-demo.gif

Set Shell "bash"
Set FontSize 22
Set Width 1500
Set Height 960
Set Padding 20
Set Margin 0
Set BorderRadius 8
Set Framerate 30
Set Theme "Catppuccin Mocha"
Set CursorBlink false

Require "./target/release/twatch"

Hide
Type "./target/release/twatch --demo -n 0.2"
Enter
Wait 2s

Type "Backspace"
Wait 800ms

Type "/"
Wait 150ms
Type "diff"
Wait 900ms
Enter
Wait 800ms

Type "s"
Wait 600ms

Type "S"
Wait 1s

Type "q"
Wait 400ms
Type "y"
Wait 800ms
