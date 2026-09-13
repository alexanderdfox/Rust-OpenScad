// User functions + list comprehension style ranges
function area(r) = 3.14159 * r * r;

r = 5;
echo(area(r)); // no-op but parsed

for (a = [0 : 60 : 300]) {
    rotate([0, 0, a])
        translate([15, 0, 0])
            sphere(r = 2);
}
