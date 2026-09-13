// Simple box with cylindrical holes
difference() {
    cube([30, 20, 10], center = true);
    translate([8, 0, 0]) cylinder(h = 20, r = 3, center = true);
    translate([-8, 0, 0]) cylinder(h = 20, r = 3, center = true);
}
