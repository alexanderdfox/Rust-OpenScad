// Variables, for-loops, and modules
size = 10;
spacing = 12;

module peg(h = 5, r = 2) {
    cylinder(h = h, r = r);
}

for (i = [0 : 2]) {
    translate([i * spacing, 0, 0])
        peg(h = size / 2, r = 2);
}

cube([size, size, 2]);
