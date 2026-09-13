// linear_extrude from 2D
linear_extrude(height = 8)
    circle(r = 10);

translate([30, 0, 0])
linear_extrude(height = 5)
    square(12, center = true);
