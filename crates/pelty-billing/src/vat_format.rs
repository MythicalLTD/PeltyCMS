//! Offline VAT number format validation (EU + UK), run before VIES.
//! Checksums are verified for RO, DE, GB, IT, BE, NL, PL, ES-free formats get
//! only a length/charset check. This does NOT prove the number is registered;
//! VIES remains the authority.

fn digits(s: &str) -> Option<Vec<u32>> {
    s.chars().map(|c| c.to_digit(10)).collect()
}

fn all_digits(s: &str, lens: &[usize]) -> bool {
    lens.contains(&s.len()) && s.chars().all(|c| c.is_ascii_digit())
}

/// `cc` is the 2-letter prefix (GR accepted as EL), `num` the rest, normalised uppercase.
pub fn is_valid_format(cc: &str, num: &str) -> bool {
    match cc {
        "RO" => ro(num),
        "DE" => de(num),
        "GB" | "XI" => gb(num),
        "IT" => it(num),
        "BE" => be(num),
        "NL" => {
            num.len() == 12
                && num.as_bytes()[9] == b'B'
                && all_digits(&num[..9], &[9])
                && all_digits(&num[10..], &[2])
        }
        "PL" => pl(num),
        "AT" => num.len() == 9 && num.starts_with('U') && all_digits(&num[1..], &[8]),
        "BG" => all_digits(num, &[9, 10]),
        "CY" => {
            num.len() == 9 && all_digits(&num[..8], &[8]) && num.as_bytes()[8].is_ascii_uppercase()
        }
        "CZ" => all_digits(num, &[8, 9, 10]),
        "DK" | "FI" | "HU" | "LU" | "MT" | "SI" => all_digits(num, &[8]),
        "EE" | "EL" | "GR" | "PT" => all_digits(num, &[9]),
        "ES" => num.len() == 9 && num.chars().all(|c| c.is_ascii_alphanumeric()),
        "FR" => {
            num.len() == 11
                && num.chars().all(|c| c.is_ascii_alphanumeric())
                && all_digits(&num[2..], &[9])
        }
        "HR" => all_digits(num, &[11]),
        "IE" => (8..=9).contains(&num.len()) && num.chars().all(|c| c.is_ascii_alphanumeric()),
        "LT" => all_digits(num, &[9, 12]),
        "LV" => all_digits(num, &[11]),
        "SE" => all_digits(num, &[12]) && num.ends_with("01"),
        "SK" => all_digits(num, &[10]),
        _ => false,
    }
}

fn ro(num: &str) -> bool {
    if !(2..=10).contains(&num.len()) {
        return false;
    }
    let Some(d) = digits(num) else { return false };
    let key = [7, 5, 3, 2, 1, 7, 5, 3, 2];
    let body = &d[..d.len() - 1];
    let pad = 9 - body.len();
    let sum: u32 = body.iter().enumerate().map(|(i, x)| x * key[pad + i]).sum();
    let c = (sum * 10) % 11 % 10;
    c == d[d.len() - 1]
}

fn de(num: &str) -> bool {
    if !all_digits(num, &[9]) {
        return false;
    }
    let d = digits(num).unwrap_or_default();
    let mut p = 10u32;
    for x in &d[..8] {
        let mut s = (x + p) % 10;
        if s == 0 {
            s = 10;
        }
        p = (2 * s) % 11;
    }
    let c = (11 - p) % 10;
    c == d[8]
}

fn gb(num: &str) -> bool {
    if num.starts_with("GD") || num.starts_with("HA") {
        return all_digits(&num[2..], &[3]);
    }
    if !all_digits(num, &[9, 12]) {
        return false;
    }
    let d = digits(&num[..9]).unwrap_or_default();
    let s: i32 = d[..7]
        .iter()
        .enumerate()
        .map(|(i, x)| *x as i32 * (8 - i as i32))
        .sum::<i32>()
        + (d[7] * 10 + d[8]) as i32;
    s % 97 == 0 || (s + 55) % 97 == 0
}

fn it(num: &str) -> bool {
    if !all_digits(num, &[11]) {
        return false;
    }
    let d = digits(num).unwrap_or_default();
    let mut s = 0;
    for (i, x) in d[..10].iter().enumerate() {
        if i % 2 == 0 {
            s += x;
        } else {
            let y = x * 2;
            s += if y > 9 { y - 9 } else { y };
        }
    }
    (10 - s % 10) % 10 == d[10]
}

fn be(num: &str) -> bool {
    let n = if num.len() == 9 {
        format!("0{num}")
    } else {
        num.to_string()
    };
    if !all_digits(&n, &[10]) {
        return false;
    }
    let base: u64 = n[..8].parse().unwrap_or(0);
    let chk: u64 = n[8..].parse().unwrap_or(0);
    97 - base % 97 == chk
}

fn pl(num: &str) -> bool {
    if !all_digits(num, &[10]) {
        return false;
    }
    let d = digits(num).unwrap_or_default();
    let w = [6, 5, 7, 2, 3, 4, 5, 6, 7];
    let s: u32 = d[..9].iter().zip(w).map(|(a, b)| a * b).sum();
    s % 11 == d[9]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checksums() {
        assert!(ro("18547290"));
        assert!(!ro("18547291"));
        assert!(de("136695976"));
        assert!(!de("136695977"));
        assert!(gb("980780684"));
        assert!(!gb("980780685"));
        assert!(it("00743110157"));
        assert!(!it("00743110158"));
        assert!(be("0403019261"));
        assert!(pl("5260250995"));
        assert!(!pl("5260250996"));
        assert!(is_valid_format("NL", "004495445B01"));
        assert!(is_valid_format("AT", "U12345678"));
        assert!(!is_valid_format("XX", "123"));
    }
}
