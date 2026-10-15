//! V3 discriminator: does the quarter model's mouth defect need the tangent junction (the symx
//! plane x=0 touches the hole cylinder along a generator), or does the graze lattice row suffice?
//!
//! Half plate x∈[−10,10] × y∈[0,10], hole r=1 fully through: the hole edge crosses the y=0
//! symmetry plane transversally (normal corner at (±1, 0)) while the Kirsch point (0, 1) keeps
//! the same graze-class y-rows the quarter model has at this h (rows 0.98592 / 1.12676 straddle
//! y=1 at h=0.14; y=1 lies on a row at h=0.1). BCs by region language only. The whole-face ux
//! anchor's end bias is neutralized by a paired run at aligned h on the identical model.
//!
//! Outcome rule (fixed before running):
//! - half h=0.14 max centroid σxx near (0,1) ≈ 30 → the defect needs the tangent junction.
//! - also elevated → the row/feature straddle alone breaks the field (watch σyy at the (±1,0)
//!   crossings too: with h=0.14 the x-rows 0.90909 / 1.04895 straddle x=±1).

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
    let fx = 10.0 * 10.0 * t;
    format!(
        "name \"Plate with hole V3, half model\"\n\
         custommaterial steel200 e=200000 nu=0.3 yield=250 density=7.85\n\
         material steel200\n\
         box plate size=(20, 10, {t}) at=(0, 5, {t2})\n\
         cylinder hole r=1 h={hh} at=(0, 0, {t2}) subtract\n\
         region symy = plate.ny\n\
         region symz = plate.nz\n\
         region symz2 = plate.pz\n\
         region anchor = plate.nx\n\
         region far = plate.px\n\
         support ry symy fixed=y\n\
         support rz symz fixed=z\n\
         support rz2 symz2 fixed=z\n\
         support rx anchor fixed=x\n\
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
    let f = |p: [f64; 3]| expr::eval(&body, p).v;
    let (mut lin, _) = fieldsmith_mesh::stuff(f, bnd.lo, bnd.hi, h);
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
        "elements {}  nodes {}  nverts {}",
        mesh.tets.len(),
        mesh.nodes.len(),
        mesh.nverts
    );

    let nn = mesh.nodes.len();
    let mut fixed = vec![0u8; nn];
    for s in &d.supports {
        let fs = region_facets(&d, &s.region, &mesh, &facets, h).unwrap();
        let mask = (s.fixed[0] as u8) | (s.fixed[1] as u8) << 1 | (s.fixed[2] as u8) << 2;
        for &fi in &fs {
            for &n in &facets[fi].nodes {
                fixed[n as usize] |= mask;
            }
        }
    }
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
                // The instrument predates distributed loads (M11); it reproduces V3's
                // point/pressure case only. Refuse rather than silently approximate.
                LoadKind::Distributed { .. } => {
                    panic!("v3half does not implement distributed loads ({})", l.id)
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
        fixed,
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

    // ---- element stress at centroid, σxx and σyy tops.
    let st: Vec<[f64; 2]> = (0..mesh.tets.len())
        .map(|e| {
            let s = element_stress(&mesh, &u, e, material.young[e], nu, centroid_xi);
            [s[0], s[1]]
        })
        .collect();
    let mut top_x: Vec<(f64, usize)> = st.iter().enumerate().map(|(e, s)| (s[0], e)).collect();
    top_x.sort_by(|a, b2| b2.0.partial_cmp(&a.0).unwrap());
    let mut top_y: Vec<(f64, usize)> = st.iter().enumerate().map(|(e, s)| (s[1], e)).collect();
    top_y.sort_by(|a, b2| b2.0.partial_cmp(&a.0).unwrap());
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
    println!("== element σxx at centroid, top 30 (quarter model at h=0.14 peaks 45.48) ==");
    for &(sxx, e) in top_x.iter().take(30) {
        let c = centroid(e);
        println!(
            "  e{:6} centroid=({:8.5},{:8.5},{:7.4}) vol={:.3e} sxx={:9.3} (Kt {:.2})",
            e,
            c[0],
            c[1],
            c[2],
            mesh.tet_volume(e).abs(),
            sxx,
            sxx / 11.111
        );
    }
    println!("== element σyy at centroid, top 5 (catches defects at the (±1,0) crossings) ==");
    for &(syy, e) in top_y.iter().take(5) {
        let c = centroid(e);
        println!(
            "  e{:6} centroid=({:8.5},{:8.5},{:7.4}) vol={:.3e} syy={:9.3}",
            e,
            c[0],
            c[1],
            c[2],
            mesh.tet_volume(e).abs(),
            syy
        );
    }

    // ---- recovered nodal σxx top 6 with per-incident-element breakdown.
    let s = nodal_stress(&mesh, &u, &material.young, nu);
    println!("== recovered σxx top 6, per-incident-element breakdown ==");
    let mut topn: Vec<(usize, f64)> = s.iter().enumerate().map(|(i, s)| (i, s[0])).collect();
    topn.sort_by(|a, b2| b2.1.partial_cmp(&a.1).unwrap());
    for &(i, _) in topn.iter().take(6) {
        let p = mesh.nodes[i];
        let kind = if i < mesh.nverts { "vert" } else { "mid " };
        println!(
            "-- top n{i} ({kind}) p=({:.5},{:.5},{:.4}) recovered sxx={:.3} u=({:+.3e},{:+.3e},{:+.3e})",
            p[0], p[1], p[2], s[i][0], u[3 * i], u[3 * i + 1], u[3 * i + 2]
        );
        let mut count = 0usize;
        for (e, sc) in st.iter().enumerate() {
            let tt = &mesh.tets[e];
            let Some(a) = tt.iter().position(|&x| x as usize == i) else {
                continue;
            };
            count += 1;
            let sn = element_stress(&mesh, &u, e, material.young[e], nu, pts[a]);
            let c = centroid(e);
            println!(
                "    e{:6} a={} centroid=({:8.5},{:8.5},{:7.4}) vol={:.3e} sxx@node={:9.3} sxx@cent={:9.3}",
                e, a, c[0], c[1], c[2], mesh.tet_volume(e).abs(), sn[0], sc[0]
            );
        }
        println!("    incident elements: {count}");
    }

    // ---- Kirsch anchors on free-edge values: 3σ = 30 at (0,1); −σ = −10 (σyy) at (±1,0).
    println!("== recovered stress at the Kirsch anchor points (nearest node) ==");
    let locate = |tp: [f64; 3]| -> (usize, f64) {
        let mut best = (usize::MAX, f64::MAX);
        for (i, p) in mesh.nodes.iter().enumerate() {
            let d2 = (0..3).map(|k| (p[k] - tp[k]).powi(2)).sum::<f64>();
            if d2 < best.1 {
                best = (i, d2);
            }
        }
        (best.0, best.1.sqrt())
    };
    for (name, tp, expect) in [
        ("edge(0,1,0)  sxx→30", [0.0, 1.0, 0.0], 30.0),
        ("edge(0,1,t/2) sxx→30", [0.0, 1.0, t / 2.0], 30.0),
        ("edge(0,1,t)  sxx→30", [0.0, 1.0, t], 30.0),
        ("edge(1,0,t/2) syy→−10", [1.0, 0.0, t / 2.0], -10.0),
        ("edge(−1,0,t/2) syy→−10", [-1.0, 0.0, t / 2.0], -10.0),
    ] {
        let (i, dist) = locate(tp);
        let p = mesh.nodes[i];
        println!(
            "  {name:20} n{i:6} p=({:.5},{:.5},{:.4}) d={:.3} sxx={:+9.3} syy={:+9.3}",
            p[0], p[1], p[2], dist, s[i][0], s[i][1]
        );
        let _ = expect;
    }

    // ---- decay along the y-axis: σxx(0,y) = 10(1 + 0.5/y² + 1.5/y⁴), nearest centroid.
    println!("== σxx decay along x≈0 (nearest centroid vs on-axis Kirsch) ==");
    for &y in [1.0563f64, 1.1268, 1.2676, 1.4085, 2.0].iter() {
        let tp = [0.0, y, t / 2.0];
        let mut best = (usize::MAX, f64::MAX);
        for (e, c) in (0..mesh.tets.len()).map(|e| (e, centroid(e))) {
            let d2 = (0..3).map(|k| (c[k] - tp[k]).powi(2)).sum::<f64>();
            if d2 < best.1 {
                best = (e, d2);
            }
        }
        let (e, d2) = best;
        let c = centroid(e);
        let expected = 10.0 * (1.0 + 0.5 / (y * y) + 1.5 / (y * y * y * y));
        println!(
            "  y={:7.4} centroid=({:8.5},{:8.5},{:7.4}) d={:.4} sxx={:8.3} expected {:8.3}",
            y,
            c[0],
            c[1],
            c[2],
            d2.sqrt(),
            st[e][0],
            expected
        );
    }
}
