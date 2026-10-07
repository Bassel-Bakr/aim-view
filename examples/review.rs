//! Reviews one request (src/review.rs `ReviewRequest`, as JSON in a file) and prints the outcome, for checks outside
//! the app: `cargo run --profile quick --example review -- <request.json>`.

/// Reads the request file named on the command line and writes the review's JSON ({report} or {error}) to standard
/// output.
fn main() {
    let path = std::env::args().nth(1).expect("usage: review <request.json>");
    let request = std::fs::read(&path).expect("the request could not be read");
    std::io::Write::write_all(&mut std::io::stdout(), &aimview::review::review_json(&request)).unwrap();
}
