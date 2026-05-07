Output img/demo_cli.gif

Set Shell "bash"
Set FontSize 20
Set Width 1480
Set Height 720
Set Framerate 30
Set Padding 24
Set CursorBlink true
Set TypingSpeed 40ms

Require "./target/debug/twatch"

# 1st
Type "# Batch execution with specified virtual terminal size"
Enter
Sleep 1s
Type "./target/debug/twatch --batch-size 80,20 -b htop"
Enter
Sleep 8s
Ctrl+C
Sleep 1s

# 2nd
Type "# The specified range is extracted and executed in batches using the specified virtual terminal size."
Enter
Sleep 1s
Type "./target/debug/twatch --batch-size 80,20 --batch-crop 0,10,80,10 -b htop"
Enter
Sleep 8s
Ctrl+C
Sleep 1s
