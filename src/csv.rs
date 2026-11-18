//! Writing tables out as CSV files that Excel, Numbers and Google Sheets open
//! cleanly.

/// One cell: quoted when it needs to be. Text a spreadsheet would run as a
/// formula (a name starting with `=`, say) is kept as text.
fn field(value: &str) -> String {
    let value = if value.starts_with(['=', '+', '@']) { format!("'{value}") } else { value.to_owned() };
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

/// A whole file: a header row, then the rows. Starts with a byte-order mark
/// so Excel reads accented names correctly; lines end in CRLF as the format
/// specifies.
pub fn table(header: &[&str], rows: &[Vec<String>]) -> String {
    let mut out = String::from("\u{feff}");
    let line = |cells: Vec<String>| cells.join(",") + "\r\n";
    out += &line(header.iter().map(|h| field(h)).collect());
    for row in rows {
        out += &line(row.iter().map(|c| field(c)).collect());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_only_what_needs_it() {
        let rows = vec![vec!["Ann".into(), "Leith, Edinburgh".into(), "Said \"hi\"".into(), "=1+1".into()]];
        assert_eq!(table(&["Name", "From", "Note", "X"], &rows), "\u{feff}Name,From,Note,X\r\nAnn,\"Leith, Edinburgh\",\"Said \"\"hi\"\"\",'=1+1\r\n");
    }
}
