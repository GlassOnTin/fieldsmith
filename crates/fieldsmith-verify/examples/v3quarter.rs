//! V3 quarter-model diagnosis instrument (M19). The quarter document was never committed; this
//! reconstruction is validated against the peaks V3.md records (45.48 MPa at h = 0.14, 31.24 at
//! h = 0.2, 45.59 at h = 0.07, 31.58 at h = 0.1) — a reconstruction that lands on those numbers
//! is the quarter model. Then it names the mouth defect from the mesh itself: which nodes are
//! warped onto the junction, which are pinned, the wedge's element volumes, and where the peak
//! stress sits.
//!
//! Quarter document: plate x∈[0,10] × y∈[0,10] × t=0.14, hole r=1 centred at the corner (0,0)
//! subtract, ux=0 on the x=0 plane (symmetry), uy=0 on y=0, uz=0 on both z faces, σgross traction
//! (14 N) on x=10. The x=0 symmetry plane passes through the hole's axis and meets the hole wall
//! along the generators (0,±1,z); the mouth at (0,1) is where the wall leaves the plane, the arc
//! there running parallel to the x-direction lattice rows.

use fenics_element::{FiniteElement, P2Tetrahedron};
use fieldsmith_core::material_of;
use fieldsmith_core::regions::{region_facets, Facet};
use fieldsmith_doc::LoadKind;
use fieldsmith_field::expr;
use fieldsmith_solve::p2::face_nodes;
use fieldsmith_solve::stress::element_stress;
use fieldsmith_solve::{nodal_stress, solve, Material, P2Mesh, Problem, SolverOptions};
use fieldsmith_verify::doc;
use std::time::Instant;

fn document(t: f64) -> String {
    let fx = 10.0 * 10.0 * t; // σgross × quarter width × thickness
    format!(
        "name \"Plate with hole V3, quarter model\"\n\
         custommaterial steel200 e=200000 nu=0.3 yield=250 density=7.85\n\
         material steel200\n\
         box plate size=(10, 10, {t}) at=(5, 5, {t2})\n\
         cylinder hole r=1 h={hh} at=(0, 0, {t2}) subtract\n\
         region symx = plate.nx\n\
         region symy = plate.ny\n\
         region symz = plate.nz\n\
         region symz2 = plate.pz\n\
         region far = plate.px\n\
         support rx symx fixed=x\n\
         support ry symy fixed=y\n\
         support rz symz fixed=z\n\
         support rz2 symz2 fixed=z\n\
         load pull far force=({fx}, 0, 0)\n",
        t2 = t / 2.0,
        hh = 4.0 * t,
    )
}

fn main() {
    let t: f64 = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "0.14".into())
        .parse()
        .unwrap();
    let h: f64 = std::env::args()
        .nth(2)
        .unwrap_or_else(|| t.to_string())
        .parse()
        .unwrap();
    println!("thickness t={t}, mesh h={h}");
    let d = doc(&document(t));
    let body = expr::compile_body(&d);
    let bnd = expr::bounds(&body);
    println!(
        "bounds lo=({:.5},{:.5},{:.6}) hi=({:.5},{:.5},{:.6})",
        bnd.lo[0], bnd.lo[1], bnd.lo[2], bnd.hi[0], bnd.hi[1], bnd.hi[2]
    );
    let f = |p: [f64; 3]| expr::eval(&body, p).v;
    let (mut lin, lat) = fieldsmith_mesh::stuff(f, bnd.lo, bnd.hi, h);
    lin.compact();
    let mesh = P2Mesh::from_linear(&lin.verts, &lin.tets);
    let boundary = lin.boundary();
    let facets: Vec<Facet> = boundary
        .iter()
        .map(|(fc, t)| {
            let tet = &mesh.tets[*t as usize];
            let local = fc.map(|v| {
                tet[..4]
                    .iter()
                    .position(|&x| x == v)
                    .expect("face vertex in tet")
            });
            Facet {
                nodes: face_nodes(tet, local),
            }
        })
        .collect();
    println!(
        "elements {}  nodes {}  nverts {}  boundary facets {}",
        mesh.tets.len(),
        mesh.nodes.len(),
        mesh.nverts,
        facets.len()
    );
    println!(
        "lattice h=({:.6},{:.6},{:.6}) origin=({:.6},{:.6},{:.6}) n=({},{},{})",
        lat.h[0],
        lat.h[1],
        lat.h[2],
        lat.origin[0],
        lat.origin[1],
        lat.origin[2],
        lat.n[0],
        lat.n[1],
        lat.n[2]
    );

    let nn = mesh.nodes.len();
    let mut fixed = vec![0u8; nn];
    for s in &d.supports {
        let fs = region_facets(&d, &s.region, &mesh, &facets, h).unwrap();
        let mask = (s.fixed[0] as u8) | (s.fixed[1] as u8) << 1 | (s.fixed[2] as u8) << 2;
        let mut pinned = 0usize;
        for &fi in &fs {
            for &n in &facets[fi].nodes {
                if fixed[n as usize] == 0 {
                    pinned += 1;
                }
                fixed[n as usize] |= mask;
            }
        }
        println!(
            "support '{}': {} facets, {} nodes pinned (cumulative-new)",
            s.region,
            fs.len(),
            pinned
        );
    }
    // Pinned-ux nodes that are not exactly on the plane x=0: the pin's own claim.
    let off_plane = (0..nn)
        .filter(|&i| fixed[i] & 1 != 0 && mesh.nodes[i][0].abs() > 1e-9)
        .count();
    println!("pinned-ux nodes off the x=0 plane (|x|>1e-9): {off_plane}");

    let mat = material_of(&d).unwrap();
    let material = Material {
        young: vec![mat.e; mesh.tets.len()],
        poisson: mat.nu,
    };
    let mut force = vec![0.0; nn * 3];
    for l in &d.loads {
        let fs = region_facets(&d, &l.region, &mesh, &facets, h).unwrap();
        let area: f64 = fs.iter().map(|&i| facets[i].area(&mesh)).sum();
        for &fi in &fs {
            let fct = &facets[fi];
            let a = fct.area(&mesh);
            let tv = match l.kind {
                LoadKind::Force { vector } => vector.map(|c| c / area),
                LoadKind::Pressure { mpa } => fct.normal(&mesh).map(|c| -mpa * c),
                LoadKind::Distributed { .. } => {
                    panic!("v3quarter does not implement distributed loads ({})", l.id)
                }
            };
            for &n in &fct.nodes[3..] {
                for c in 0..3 {
                    force[3 * n as usize + c] += tv[c] * a / 3.0;
                }
            }
        }
    }

    let problem = Problem {
        mesh: mesh.clone(),
        material: material.clone(),
        fixed: fixed.clone(),
        force,
    };
    let t0 = Instant::now();
    let (u, r) = solve(&problem, SolverOptions::default()).expect("P2 solve");
    println!(
        "solve: iters {} rel_residual {:.3e} ({:.1}s)",
        r.iterations,
        r.rel_residual,
        t0.elapsed().as_secs_f64()
    );

    let nu = mat.nu;
    let pts = P2Tetrahedron.nodal_coordinates();
    let mut centroid_xi = [0.0f64; 3];
    for &pk in pts.iter().take(4) {
        for (j, x) in centroid_xi.iter_mut().enumerate() {
            *x += pk[j] / 4.0;
        }
    }

    // ---- element σxx at centroid, top 25 -------------------------------------------------------
    let st: Vec<f64> = (0..mesh.tets.len())
        .map(|e| element_stress(&mesh, &u, e, material.young[e], nu, centroid_xi)[0])
        .collect();
    let s = nodal_stress(&mesh, &u, &material.young, nu);
    let mut top_x: Vec<(f64, usize)> = st.iter().enumerate().map(|(e, s)| (*s, e)).collect();
    top_x.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let centroid = |e: usize| {
        // p(ξcentroid) under the quadratic map: N_corner(ξc) = −1/8, N_mid(ξc) = +1/4
        // (ξc = (1/4,1/4,1/4,1/4), equal barycentric weight; weights sum to 1). A flat /4 over
        // all 10 node ids overweights the midside nodes ~10× and inflates printed centroids.
        let mut c = [0.0f64; 3];
        for (i, &v) in mesh.tets[e].iter().enumerate() {
            let w = if i < 4 { -0.125 } else { 0.25 };
            for (j, x) in c.iter_mut().enumerate() {
                *x += mesh.nodes[v as usize][j] * w;
            }
        }
        c
    };
    let hhmax = lat.h[0].max(lat.h[1]).max(lat.h[2]);
    println!(
        "== element σxx at centroid, top 25 (V3.md quarter record: 45.48 at h=0.14, 31.24 at h=0.2) =="
    );
    for &(sxx, e) in top_x.iter().take(25) {
        let c = centroid(e);
        println!(
            "  e{:6} centroid=({:8.5},{:8.5},{:7.4}) r={:7.4} vol/h3={:7.3} sxx={:9.3}",
            e,
            c[0],
            c[1],
            c[2],
            (c[0] * c[0] + c[1] * c[1]).sqrt(),
            mesh.tet_volume(e).abs() / (hhmax * hhmax * hhmax),
            sxx
        );
    }

    // ---- mouth node census: the junction of the wall with the plane at (0,1) -------------------
    // Coordinates, classification (lattice row / warped onto the cylinder / midpoint), solution.
    let on_lattice = |p: [f64; 3]| -> Option<[usize; 3]> {
        let mut idx = [0usize; 3];
        for a in 0..3 {
            let s = (p[a] - lat.origin[a]) / lat.h[a];
            let r = s.round();
            if (s - r).abs() > 1e-7 {
                return None;
            }
            idx[a] = r as usize;
        }
        Some(idx)
    };
    let on_cylinder =
        |p: [f64; 3]| -> bool { ((p[0] * p[0] + p[1] * p[1]).sqrt() - 1.0).abs() < 1e-6 };
    println!("== nodes near the mouth (x<0.30, y in [0.80, 1.40]) ==");
    for i in 0..nn {
        let p = mesh.nodes[i];
        if p[0] < 0.30 && (0.80..1.40).contains(&p[1]) {
            let kind = if i >= mesh.nverts {
                "mid"
            } else if on_lattice(p).is_some() {
                if on_cylinder(p) {
                    "row+arc"
                } else {
                    "row"
                }
            } else if on_cylinder(p) {
                "warped-arc"
            } else {
                "warped-other"
            };
            println!(
                "  n{:6} ({:9.6},{:9.6},{:8.4}) {:12} pinned={:03b} u=({:+.3e},{:+.3e},{:+.3e})",
                i,
                p[0],
                p[1],
                p[2],
                kind,
                fixed[i],
                u[3 * i],
                u[3 * i + 1],
                u[3 * i + 2]
            );
        }
    }

    // ---- duplicate-coordinate census (two vertices sharing one position) ------------------------
    let mut pos_map: std::collections::HashMap<[i64; 3], Vec<usize>> =
        std::collections::HashMap::new();
    for (i, p) in mesh.nodes.iter().enumerate() {
        let k = p.map(|c| (c * 1e9).round() as i64);
        pos_map.entry(k).or_default().push(i);
    }
    let dups: Vec<&Vec<usize>> = pos_map.values().filter(|g| g.len() > 1).collect();
    println!("== duplicate-coordinate groups: {} ==", dups.len());
    for g in dups.iter().take(20) {
        let p = mesh.nodes[g[0]];
        println!(
            "  p=({:9.6},{:9.6},{:8.4}) nodes {:?} pinned={:?}",
            p[0],
            p[1],
            p[2],
            g,
            g.iter().map(|&i| fixed[i]).collect::<Vec<_>>()
        );
    }

    // ---- sliver census: thin tets touching a pinned node ----------------------------------------
    let pinned_nodes: Vec<usize> = (0..nn).filter(|&i| fixed[i] != 0).collect();
    let is_pinned = |v: usize| pinned_nodes.binary_search(&v).is_ok();
    println!(
        "== tets by volume (h-normalised), thinnest 20, and thinnest 10 touching a pinned node =="
    );
    let mut by_vol: Vec<(f64, usize)> = (0..mesh.tets.len())
        .map(|e| (mesh.tet_volume(e).abs() / (hhmax * hhmax * hhmax), e))
        .collect();
    by_vol.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    for &(vr, e) in by_vol.iter().take(20) {
        let c = centroid(e);
        println!(
            "  e{:6} vol/h3={:8.4} centroid=({:8.5},{:8.5},{:7.4}) sxx={:8.3}",
            e, vr, c[0], c[1], c[2], st[e]
        );
    }
    let mut pv: Vec<(f64, usize)> = by_vol
        .iter()
        .filter(|&&(_, e)| mesh.tets[e].iter().any(|&v| is_pinned(v as usize)))
        .copied()
        .collect();
    println!("  (tets touching a pinned node: {})", pv.len());
    pv.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    for &(vr, e) in pv.iter().take(10) {
        let c = centroid(e);
        println!(
            "  e{:6} vol/h3={:8.4} centroid=({:8.5},{:8.5},{:7.4}) sxx={:8.3} nodes={:?}",
            e, vr, c[0], c[1], c[2], st[e], mesh.tets[e]
        );
    }

    // ---- the mouth node itself: incident tets, neighbours' ux -----------------------------------
    println!(
        "== mouth region u profile: nodes sorted by x for y≈1 (nearest to (0,1,t/2) first) =="
    );
    let mut near: Vec<(f64, usize)> = (0..nn)
        .filter(|&i| {
            let p = mesh.nodes[i];
            p[0] < 0.30 && (0.95..1.10).contains(&p[1]) && (p[2] - t / 2.0).abs() < 0.02
        })
        .map(|i| (mesh.nodes[i][0], i))
        .collect();
    near.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    for &(_, i) in near.iter().take(15) {
        let p = mesh.nodes[i];
        let sx = s[i][0];
        println!(
            "  n{:6} ({:9.6},{:9.6},{:8.4}) pinned={:03b} ux={:+.4e} uy={:+.4e} rec.sxx={:8.3}",
            i,
            p[0],
            p[1],
            p[2],
            fixed[i],
            u[3 * i],
            u[3 * i + 1],
            sx
        );
    }
    // The mouth node's incident tets in full, so the wedge's own cells are named.
    if let Some(&(mx, mi)) = near.first() {
        println!(
            "== incident tets of the mouth node n{} (x={:.6}) ==",
            mi, mx
        );
        for (e, tt) in mesh.tets.iter().enumerate() {
            if tt.iter().any(|&v| v as usize == mi) {
                let c = centroid(e);
                let volr = mesh.tet_volume(e).abs() / (hhmax * hhmax * hhmax);
                println!(
                    "  e{:6} vol/h3={:8.4} centroid=({:8.5},{:8.5},{:7.4}) sxx={:8.3}",
                    e, volr, c[0], c[1], c[2], st[e]
                );
                for &v in tt.iter() {
                    let p2 = mesh.nodes[v as usize];
                    println!(
                        "      v{:5} ({:9.6},{:9.6},{:8.4}) pinned={:03b} ux={:+.4e}",
                        v,
                        p2[0],
                        p2[1],
                        p2[2],
                        fixed[v as usize],
                        u[3 * v as usize]
                    );
                }
            }
        }
    }
}
