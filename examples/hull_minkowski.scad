// hull and minkowski
hull() {
    translate([0, 0, 0]) sphere(3);
    translate([20, 0, 0]) sphere(3);
    translate([10, 15, 0]) sphere(3);
}

translate([0, 30, 0])
minkowski() {
    cube([10, 10, 2], center = true);
    sphere(1);
}
