# wasm-squeeze

`wasm-squeeze` is an [UPX](https://github.com/upx/upx)-like tool to compress [WASM-4](https://wasm4.org/) cartridges and embed the decompressor into the binary for it to decompress itself.

Generally speaking, this tool allows to **reduce cartridge's file size**, so that it can be stuffed with some more code and data before reaching 64KiB file size boundary of WASM-4! However right now it doesn't reduce RAM consumption.

## Installation

Use cargo for now:

```
cargo install wasm-squeeze --locked --git https://github.com/zetanumbers/wasm-squeeze.git
```

## Usage

```bash
wasm-squeeze example.wasm -o example-squeezed.wasm
```

Please note that this tool may introduce redundant information into the cartridge, so it's most probably desirable to use `wasm-opt` after the module got through `wasm-squeeze`.
You might find stdio support useful for this, just specify "-" as input or output filepaths (not specifying those works too).

## Compression benchmarks

I have compared all cartridge builds published on the official site ([back then](https://github.com/aduros/wasm4/commit/be6bc297d77592b37d1c1bd53dcbc168a06a2ce1)) processed by `wasm-opt -Oz -uim -all` and the same cartridge builds going through `wasm-squeeze` first and then `wasm-opt` with aformentioned arguments.
Negative values (size increases) were preserved, however in practice users should measure benefits and not use `wasm-squeeze` in such cases. 

Benchmarks were done on [de7e7ab](https://github.com/zetanumbers/wasm-squeeze/commit/de7e7abd87ed68fc67134cb155fa429e3e6ce889) commit:

|name|old_size|new_size|size_decrease%|
|-|-|-|-|
|wasm4-gbvgm-player|43.1 KiB|4.0 KiB|90.67|
|zoopzoop|19.8 KiB|7.3 KiB|63.07|
|lingword|38.4 KiB|14.7 KiB|61.74|
|nyancat|25.0 KiB|10.7 KiB|57.20|
|ur|40.3 KiB|17.9 KiB|55.50|
|the-legend-of-geml-awakening|45.6 KiB|21.1 KiB|53.85|
|lad2024|8.6 KiB|4.5 KiB|48.14|
|trials-of-the-dark-sea|44.2 KiB|23.4 KiB|46.94|
|miku-15|39.3 KiB|22.2 KiB|43.47|
|ioccc98flightsim|39.1 KiB|22.2 KiB|43.09|
|palette-previewer|9.1 KiB|5.4 KiB|40.10|
|formula-1|8.2 KiB|4.9 KiB|40.01|
|strikeforce|19.4 KiB|11.9 KiB|38.76|
|future-avoid|26.8 KiB|16.7 KiB|37.59|
|fuzzy-world-cup|38.8 KiB|26.0 KiB|33.19|
|presents-to-the-metal|15.7 KiB|10.8 KiB|31.01|
|first-flight|54.2 KiB|39.0 KiB|28.03|
|rubido|27.0 KiB|19.5 KiB|27.79|
|micro-quest|15.6 KiB|11.3 KiB|27.50|
|bombfighters|38.3 KiB|28.6 KiB|25.28|
|waternet|29.4 KiB|22.3 KiB|24.30|
|escape-guldur|27.4 KiB|21.0 KiB|23.55|
|punch-em-up|33.8 KiB|25.9 KiB|23.40|
|explore-the-grotto|50.8 KiB|39.1 KiB|23.09|
|racer|35.1 KiB|27.1 KiB|22.92|
|piano|8.2 KiB|6.3 KiB|22.88|
|to-the-core|18.3 KiB|14.4 KiB|21.03|
|mind-palace|28.8 KiB|22.8 KiB|20.90|
|minicraft|34.2 KiB|27.2 KiB|20.49|
|plctfarmer|40.0 KiB|32.1 KiB|19.84|
|you-will-return|42.8 KiB|34.4 KiB|19.78|
|break-it|4.5 KiB|3.6 KiB|19.63|
|wormhole|23.0 KiB|18.6 KiB|19.42|
|tictactoe|13.0 KiB|10.5 KiB|19.13|
|sound-test|34.7 KiB|28.1 KiB|18.97|
|touhou-spirits|53.3 KiB|43.3 KiB|18.62|
|zxz|18.2 KiB|15.1 KiB|17.24|
|hills-moonlight|18.6 KiB|15.4 KiB|16.99|
|samurai-revenge|34.7 KiB|29.1 KiB|16.32|
|totally-not-sumo|18.8 KiB|15.8 KiB|15.92|
|journey-to-entorus|54.6 KiB|45.9 KiB|15.91|
|disk-0-madness|52.8 KiB|44.5 KiB|15.82|
|ninja-vs-knights|56.6 KiB|49.1 KiB|13.19|
|antcopter|32.6 KiB|28.4 KiB|12.88|
|spunky|40.6 KiB|35.4 KiB|12.87|
|wasm4nia|20.2 KiB|17.7 KiB|12.57|
|wasm4-city|39.9 KiB|35.0 KiB|12.17|
|miniciv|63.7 KiB|56.1 KiB|11.97|
|sokoban|3.4 KiB|3.0 KiB|11.67|
|lumber-night|33.3 KiB|29.8 KiB|10.47|
|wumpus-hunt|17.9 KiB|16.2 KiB|9.87|
|wired|35.1 KiB|31.6 KiB|9.83|
|train|31.6 KiB|28.6 KiB|9.59|
|hotw|32.7 KiB|29.7 KiB|9.36|
|kittygame|51.2 KiB|46.4 KiB|9.32|
|dodgeball|23.7 KiB|21.6 KiB|8.64|
|snake|10.6 KiB|9.8 KiB|8.14|
|floppy-fish|14.6 KiB|13.4 KiB|7.99|
|glowfish-chess|55.5 KiB|51.2 KiB|7.84|
|iwas|54.0 KiB|49.8 KiB|7.81|
|wasm4-rpg|59.5 KiB|54.9 KiB|7.70|
|corn|8.5 KiB|7.9 KiB|7.43|
|w4f|35.9 KiB|33.5 KiB|6.77|
|big-space-iron|21.3 KiB|20.0 KiB|6.35|
|minesweeper|19.2 KiB|18.1 KiB|5.55|
|ping|23.6 KiB|22.3 KiB|5.41|
|the-romans-are-coming|40.5 KiB|38.7 KiB|4.44|
|tail-gunner|19.5 KiB|18.8 KiB|3.96|
|space-kommand|22.2 KiB|21.4 KiB|3.71|
|tankle|34.5 KiB|33.2 KiB|3.55|
|dashy-dango|44.2 KiB|42.7 KiB|3.39|
|rolly-dango|58.1 KiB|56.2 KiB|3.27|
|phantom-shift|9.7 KiB|9.4 KiB|3.16|
|wasm-wars|13.2 KiB|12.8 KiB|3.09|
|taufl|29.8 KiB|29.1 KiB|2.26|
|starshard-scavengers|65.5 KiB|64.2 KiB|1.98|
|fools-paradise|36.4 KiB|35.8 KiB|1.69|
|tankbattle|17.4 KiB|17.2 KiB|1.42|
|starfighterarena|22.3 KiB|22.1 KiB|0.84|
|hammer-joe|37.3 KiB|37.0 KiB|0.68|
|assemblio|31.8 KiB|31.6 KiB|0.51|
|raw-assembly|253 B|253 B|0|
|plasma-cube|4.8 KiB|4.8 KiB|0|
|lime-volleyball|3.7 KiB|3.7 KiB|0|
|maze-wanderer|9.7 KiB|9.7 KiB|0|
|bubblewrap|8.0 KiB|8.0 KiB|0|
|mouse-demo|1.4 KiB|1.4 KiB|0|
|2048|12.0 KiB|12.0 KiB|0|
|skipahead|529 B|529 B|0|
|snakery|17.7 KiB|17.7 KiB|0|
|mazethingie|3.3 KiB|3.3 KiB|0|
|seed-creator-showcase|3.7 KiB|3.7 KiB|0|
|tinypong|1.1 KiB|1.1 KiB|0|
|watris|2.5 KiB|2.5 KiB|0|
|sound-demo|1.5 KiB|1.5 KiB|0|
|text-input|4.7 KiB|4.7 KiB|0|
|platformer-test|1.1 KiB|1.1 KiB|0|
|meteoroids|17.6 KiB|17.6 KiB|0|
|seal-adventure|2.1 KiB|2.1 KiB|0|
|match3|2.5 KiB|2.5 KiB|0|
|one-slime-army|12.7 KiB|12.7 KiB|0|
|pong|6.7 KiB|6.7 KiB|0|
|shieldshooter|3.1 KiB|3.1 KiB|0|
|wloku|7.0 KiB|7.0 KiB|0|
|image-carousel|60.0 KiB|60.0 KiB|0|
|pocket-dust|8.1 KiB|8.1 KiB|0|
|projectron|12.2 KiB|12.2 KiB|0|
|maze|3.5 KiB|3.5 KiB|0|
|starfightercreator|13.7 KiB|13.7 KiB|0|
|endless-runner|2.5 KiB|2.5 KiB|0|
|space-delivery|11.1 KiB|11.1 KiB|0|
|pid-controller|10.0 KiB|10.1 KiB|-0.62|
|simple-space-invaders|10.2 KiB|10.5 KiB|-3.25|
|puyo|12.1 KiB|12.6 KiB|-4.03|
|lakeshooter|8.8 KiB|9.2 KiB|-4.69|
|smash-sugar-parallelepipeds|3.9 KiB|4.4 KiB|-12.03|
|simple-rocket|4.1 KiB|4.6 KiB|-12.49|
|game-of-life|2.9 KiB|3.3 KiB|-16.94|
|dont-smash-into-obstacles|2.6 KiB|3.0 KiB|-17.88|
|game-of-life-zig-edition|2.7 KiB|3.2 KiB|-19.60|

## How does it work?

To put it simply, we first compile the [`upkr`](https://github.com/exoticorn/upkr) unpacker from C to WASM and optimize it for size for it to be embedded directly into the `wasm-squeeze` binary.

While `wasm-squeeze` executes, it first analyzes the input WASM module to extract relevant information.
Then the unpacker WASM module is parsed and the input module is reencoded with functions and types from the unpacker module.
Data segments are merged into a big single segment, compressed via `upkr`, and is encoded at some offset from 0 address.
Then the preamble code for decompression is added to the WASM's special [start function](https://webassembly.github.io/spec/core/syntax/modules.html#start-function), which is created in case it doesn't exist.
This preamble code does data decompression, moves decompressed data into original position, and then does some cleanup after that.

If `wasm-squeeze` notices that cartridge's size haven't decreased, `wasm-squeeze` tries to simply passthrough the input module to the output.

## Credits

Thanks to [@exoticorn](https://github.com/exoticorn) for implementing the [`upkr`](https://github.com/exoticorn/upkr) crate, responsible for compression and decompression, designed with WASM in mind. Please check out [MicroW8](https://exoticorn.github.io/microw8/), another WebAssembly based fantasy console, which `upkr` was created for.

Thanks to [@aduros](https://github.com/aduros) for creating [WASM-4](https://wasm4.org/) project, a beautiful place where spanned such creative community.

Thanks to the folks from [@bytecodealliance](https://github.com/bytecodealliance/) for maintaining extensive for WASM manipulation `wasmparser` and `wasm-encoder` crates.
