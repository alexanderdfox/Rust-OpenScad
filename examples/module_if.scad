// User module + if
module box(s, hollow = false) {
    if (hollow) {
        difference() {
            cube(s, center = true);
            cube(s - 2, center = true);
        }
    } else {
        cube(s, center = true);
    }
}

box(20, hollow = true);
translate([30, 0, 0]) box(15);
