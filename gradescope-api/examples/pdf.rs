use gradescope_api::submission_export::pdf::SubmissionPdf;

fn main() {
    let path = "out/example1.pdf";
    let data = std::fs::read(path).unwrap();

    let pdf = SubmissionPdf::new("00000.pdf".to_owned(), &data).unwrap();
    let unmatched = pdf
        .as_unmatched(&["1.3".parse().unwrap(), "1".parse().unwrap()])
        .unwrap();
    println!("unmatched: {unmatched:?}");
}
