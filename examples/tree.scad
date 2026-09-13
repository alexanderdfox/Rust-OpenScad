// Classic OpenSCAD tree — works with openscad_rs
union() {
    cylinder(h = 30, r = 8);
    translate([0, 0, 40]) sphere(20);
}
