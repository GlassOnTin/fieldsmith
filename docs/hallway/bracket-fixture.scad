// Hallway fixture for the v0.3 round-trip card (docs/hallway-test.md).
// The shape is Fieldsmith's examples/bracket.fsm, re-authored by a second tool
// (OpenSCAD, this fixture's .scad source) so it exists as a foreign-tool 3MF a
// hallway participant imports. Units mm, datum: base corner at origin.
// Base 60 x 40 x 8 with two D 6.5 holes at (15,26) and (45,26); upright on the
// y=[0,8] strip, 60 x 40 x 8 standing, 8 mm inside-corner fillet; one D 8.5 hole
// through the upright at (30, *, 28).
$fn = 64;

o = 0.05; // overlap epsilon so subtracted holes cross both faces

// L cross-section in the (y, z) plane, extruded 60 along x.
// Fillet arc: quarter circle r=8 centred at (16, 16), from (16, 8) to (8, 16).
pts = concat(
    [[0, 0], [40, 0], [40, 8]],
    [for (k = [0:16]) [16 + 8 * cos(270 - 90 * k / 16), 16 + 8 * sin(270 - 90 * k / 16)]],
    [[8, 40], [0, 40]]
);

module bracket() {
    difference() {
        // sketch xy -> final (x, z) with the extrusion along +x
        translate([60, 0, 0]) rotate([0, -90, 0])
            linear_extrude(height = 60) polygon(pts);
        // base holes, axis z, extended past both faces
        translate([15, 26, 4]) cylinder(h = 20, r = 3.25, center = true);
        translate([45, 26, 4]) cylinder(h = 20, r = 3.25, center = true);
        // upright pin hole, axis y at (30, *, 28), extended past both faces
        translate([30, 4, 28]) rotate([90, 0, 0])
            cylinder(h = 20, r = 4.25, center = true);
    }
}

bracket();