// A Rust source file whose public surface becomes a `.ds` declaration via
// `ds add`. Run:  ds add examples/bindgen-demo/geometry.rs

pub struct Point {
    pub x: f64,
    pub y: f64,
}

pub struct Polyline {
    pub points: Vec<Point>,
    pub label: Option<String>,
}

pub fn distance(a: f64, b: f64) -> f64 {
    let dx = a - b;
    dx * dx
}
