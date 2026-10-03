//! Debug probe: print the body field along a line. `cargo run --example probe -- file.fsm x0 y0 z0 x1 y1 z1 n`
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&a[1]).unwrap();
    let doc = fieldsmith_doc::fsm::parse(&text).unwrap();
    let e = fieldsmith_field::expr::compile_body(&doc);
    let f: Vec<f64> = a[2..8].iter().map(|s| s.parse().unwrap()).collect();
    let n: usize = a[8].parse().unwrap();
    for i in 0..=n {
        let t = i as f64 / n as f64;
        let p = [
            f[0] + (f[3] - f[0]) * t,
            f[1] + (f[4] - f[1]) * t,
            f[2] + (f[5] - f[2]) * t,
        ];
        let d = fieldsmith_field::expr::eval(&e, p);
        println!(
            "{:8.3} {:8.3} {:8.3}  d={:9.4} |g|={:.3}",
            p[0],
            p[1],
            p[2],
            d.v,
            fieldsmith_field::dual::len(d.g)
        );
    }
}
