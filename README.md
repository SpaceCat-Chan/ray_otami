# ray_otami

a pure raymarcher that uses PBR stuff to make nice looking images,  
the scene can be edited in shapes.ron without needing a recompile using a bespoke file format called ron (json but rust)

## how to run
`cargo run --release`

or if you want to attach a debugger and are fine with a 4x slowdown (well, since this is the gpu branch, that isn't true at all, but whatever)

`cargo run`

you can also give arguments to make it use a file other than `shapes.ron`  
like this:

`cargo run --release -- shapes_alt.ron`
