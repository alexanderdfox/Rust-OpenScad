# openscad_rs

**Pure-Rust OpenSCAD-like modeller** using the [Manifold](https://github.com/elalish/manifold) geometry kernel — **no CGAL**.

Targets a useful subset of the [OpenSCAD CheatSheet](https://openscad.org/cheatsheet/index.html).

![version](https://img.shields.io/badge/version-0.2.0-blue) ![license](https://img.shields.io/badge/license-GPL--2.0--or--later-green)

## Features

- **CLI** — compile `.scad` → binary STL
- **GUI** — OpenSCAD-style IDE (editor + live 3D preview + console)
- **CSG** — `union` / `difference` / `intersection` via Manifold
- **Transforms** — translate, rotate, scale, mirror, hull, minkowski, resize
- **Primitives** — cube, sphere, cylinder, square, circle, polygon
- **Extrudes** — linear_extrude, rotate_extrude (common cases)
- **Language** — variables, modules, functions, for/if, list comprehensions (basic), math builtins

## GUI (recommended)

```bash
cargo run --release --bin openscad_rs_gui
```

Layout:

| Area | Purpose |
|------|---------|
| **Left** | Code editor (`.scad`) |
| **Right** | 3D preview — drag to orbit, scroll to zoom |
| **Bottom** | Console + status |

Shortcuts:

| Key | Action |
|-----|--------|
| `F5` / `F6` | Compile & preview |
| `Ctrl+O` | Open |
| `Ctrl+S` | Save |
| `Ctrl+E` | Export STL |
| `Home` | Reset camera |

Extras in the GUI:

- Auto-preview (debounced)
- Grid + RGB axes
- Wireframe toggle
- Built-in examples menu
- Colored console (errors / success)
- Warm shaded mesh (painter’s algorithm)

## CLI

```bash
cargo build --release

# Compile a file
./target/release/openscad_rs model.scad -o model.stl

# Built-in demos
./target/release/openscad_rs --demo tree -o tree.stl
./target/release/openscad_rs --demo box  -o box.stl

# Quiet mode
./target/release/openscad_rs model.scad -o out.stl -q
```

Demos: `tree` · `cube` · `difference` · `cylinder` · `box`

## CheatSheet coverage

### Syntax

| Feature | Status |
|---------|--------|
| `var = value` | ✅ |
| ternary `? :` | ✅ |
| `module` / `function` | ✅ |
| `include` / `use` | ✅ ignored (no file load yet) |
| anonymous function `(x) …` | ❌ |

### Constants & operators

| `PI` `undef` | ✅ |
| `+ - * / % ^` comparisons `&& \|\| !` | ✅ |

### Special variables

| `$fn $fa $fs $t` | ✅ |
| `$preview $children $vpr $vpt $vpd $vpf` | ✅ present (viewport mostly informational) |

### Modifiers `* ! # %`

✅ parsed (ignored for geometry)

### 2D

| `circle` `square` | ✅ |
| `polygon` | ✅ (thin extrude) |
| `text` | ❌ no font engine |
| `import` (DXF/SVG) | ❌ |
| `projection` | ⚠️ passthrough |

### 3D

| `sphere` `cube` `cylinder` | ✅ |
| `polyhedron` | ⚠️ convex hull of points |
| `linear_extrude` | ✅ (square/circle children) |
| `rotate_extrude` | ✅ (square/circle children) |
| `import` / `surface` | ❌ |

### Transformations

| `translate` `rotate` `scale` `mirror` | ✅ |
| `rotate(a, v)` axis-angle | ✅ (axis-aligned exact; general approx) |
| `hull` `minkowski` | ✅ |
| `resize` | ✅ approx via bbox scale |
| `multmatrix` | ⚠️ passthrough |
| `color` | ✅ passthrough |
| `offset` | ⚠️ passthrough |

### Boolean

| `union` `difference` `intersection` | ✅ |

### Lists & flow

| literals, index `[i]`, `.x .y .z` | ✅ |
| `[for (i=r) e]` | ✅ |
| `if` / `each` / C-style for / `let` inside lists | partial / ❌ |
| `for` `if/else` `let` / `assign` | ✅ |
| `intersection_for` `union_for` | ✅ (as for) |
| multi-var `for (i=…, j=…)` | ❌ |

### Functions

| Math (`abs`, `sin`, `norm`, `cross`, …) | ✅ |
| `concat` `lookup` `str` `chr` `ord` | ✅ |
| `version` `version_num` | ✅ |
| `search` `parent_module` | ⚠️ stub |
| Type tests (`is_undef` …) | ✅ |

### Other

| `echo` `assert` `render` | ✅ (no-op / passthrough) |
| `children()` | ⚠️ stub |

## Architecture

```
src/
  ast.rs      AST definitions
  parser.rs   Recursive-descent parser
  eval.rs     Evaluator → Manifold solids
  stl.rs      Binary STL writer
  main.rs     CLI
  bin/gui.rs  egui IDE
examples/     Sample .scad files
```

## Requirements

- Rust **1.77+** (Manifold binding)
- Optional: system OpenGL / wgpu stack for the GUI (eframe defaults)

## License

GPL-2.0-or-later · Manifold is Apache-2.0
