Docker Demo
===

Build the image:

```bash
docker build -f docker/Dockerfile -t twatch-vhs .
```

Generate the zsh rewind GIF inside the container:

```bash
docker run --rm \
  -v "$PWD:/work" \
  -w /work \
  twatch-vhs \
  vhs img/demo_tui_zsh.tape
```

The generated file will be written to:

```text
img/demo_tui_zsh.gif
```

Open a shell in the same image when you want to run `twatch` or `vhs` manually:

```bash
docker run --rm -it \
  -v "$PWD:/work" \
  -w /work \
  twatch-vhs
```
