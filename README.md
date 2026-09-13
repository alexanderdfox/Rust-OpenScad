# openscad_rs

Pure-Rust OpenSCAD-like modeller (Manifold backend, **no CGAL**).

Targets the [OpenSCAD CheatSheet](https://openscad.org/cheatsheet/index.html) feature set.

## Quick start

```bash
cargo build --release
./target/release/openscad_rs model.scad -o model.stl
./target/release/openscad_rs --demo tree -o tree.stl
```

## CheatSheet coverage

### Syntax
| Feature | Status |
|---------|--------|
| `var = value` | ✅ |
| ternary `? :` | ✅ |
| `module` / `function` | ✅ |
| `include` / `use` | ✅ ignored (no file load yet) |
| anonymous `function (x) …` | ❌ |

### Constants
| `PI` | ✅ | `undef` | ✅ |

### Operators
| `+ - * / % ^` | ✅ | comparisons | ✅ | `&& \|\| !` | ✅ |

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

### Lists
| literals, index `[i]`, `.x .y .z` | ✅ |

### List comprehensions
| `[for (i=r) e]` | ✅ |
| `if` / `each` / C-style for / `let` inside | partial / ❌ |

### Flow control
| `for` `if/else` `let`/`assign` | ✅ |
| `intersection_for` `union_for` | ✅ (as for) |
| multi-var `for (i=…, j=…)` | ❌ |

### Type tests
| `is_undef is_bool is_num is_string is_list is_function` | ✅ |

### Other
| `echo` `assert` `render` | ✅ (no-op / passthrough) |
| `children()` | ⚠️ stub |

### Functions
| Math (abs, sin, norm, cross, …) | ✅ |
| `concat lookup str chr ord` | ✅ |
| `version version_num` | ✅ |
| `search parent_module` | ⚠️ stub |

## Architecture

```
src/ast.rs parser.rs eval.rs stl.rs main.rs
examples/*.scad
```

## License

GPL-2.0-or-later · Manifold Apache-2.0
