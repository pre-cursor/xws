# Running

```shell
osascript -l JavaScript demos/demo.js
```

To convert the QuickTime movie file:

```shell
ffmpeg -i demos/demo.mov -filter_complex "[0:v] fps=12,scale=800:-1:flags=lanczos,split [a][b];[a] palettegen [p];[b][p] paletteuse" demos/demo.gif
```
