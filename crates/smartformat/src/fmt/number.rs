//! .NET standard numeric format specifiers: `B`, `C`, `D`, `E`, `F`, `G`,
//! `N`, `P`, `R`, `X` (upper/lower, optional precision), plus the empty spec
//! (general).
//!
//! Reference: .NET "Standard numeric format strings" documentation and
//! `System.Number` (`Number.Formatting.cs`, .NET Core 3.0+ IEEE-compliant
//! behavior). Digits come from the *exact* decimal expansion of the value —
//! every `f64` is a finite decimal — and are rounded here, because the two
//! numeric kinds round differently: integers round half away from zero
//! (`Number.RoundNumber`), floats round half to even (the correctly rounded
//! digits `Dragon4` hands to the formatter).
//!
//! Known divergence: .NET normalizes the significand before it derives the
//! rounding boundaries in `Grisu3`, which loses the "power of two" test that
//! widens the lower boundary, so for `2^-25` and `2^-958` .NET emits digits
//! that do not parse back to the original value. Reproducing that needs a
//! port of `Grisu3` including the cases where it gives up; `DESIGN.md` lists
//! it as a non-goal and the goldens pin it.

use super::culture::{CultureData, NumberFormat};
use super::FormatSpecError;

/// The numeric types a template value can hold (from `Value::Int`,
/// `Value::UInt` and `Value::Float`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Number {
    Int(i64),
    UInt(u64),
    Float(f64),
}

impl Number {
    /// The magnitude and sign of an integral value.
    fn integer(self) -> Option<(u64, bool)> {
        match self {
            Number::Int(v) => Some((v.unsigned_abs(), v < 0)),
            Number::UInt(v) => Some((v, false)),
            Number::Float(_) => None,
        }
    }
}

/// The message .NET's `FormatException` carries for a specifier that is not
/// valid at all (`System.SR.Argument_BadFormatSpecifier`).
pub const INVALID_SPEC_MESSAGE: &str = "Format specifier was invalid.";

/// Formats `n` with a .NET *standard* numeric format spec (`""`, `"N2"`,
/// `"x8"`, …), producing byte-identical output to .NET's
/// `n.ToString(spec, culture)`.
///
/// Custom patterns (anything that isn't a single standard specifier letter
/// plus optional precision digits) return [`FormatSpecError::Unsupported`].
/// `D`/`X` applied to a `Float` return [`FormatSpecError::Invalid`], as in
/// .NET.
pub fn format_number(
    n: Number,
    spec: &str,
    culture: &CultureData,
) -> Result<String, FormatSpecError> {
    let info = &culture.number;

    // .NET returns these symbols from `Number.FormatDouble` before it even
    // parses the specifier, so the specifier cannot make them fail.
    if let Number::Float(v) = n {
        if v.is_nan() {
            return Ok(info.nan_symbol.to_owned());
        }
        if v.is_infinite() {
            return Ok(if v.is_sign_negative() {
                info.negative_infinity_symbol
            } else {
                info.positive_infinity_symbol
            }
            .to_owned());
        }
    }

    let (fmt, precision) = parse_spec(spec)?;

    match (fmt.to_ascii_uppercase(), n.integer()) {
        ('D', Some((magnitude, negative))) => Ok(int_to_dec_str(
            magnitude,
            negative,
            precision.unwrap_or(0),
            info.negative_sign,
        )),
        ('X', Some((magnitude, negative))) => Ok(int_to_radix_str(
            magnitude,
            negative,
            16,
            precision.unwrap_or(0),
            fmt == 'X',
        )),
        ('B', Some((magnitude, negative))) => Ok(int_to_radix_str(
            magnitude,
            negative,
            2,
            precision.unwrap_or(0),
            false,
        )),
        // `D`, `X` and `B` are integer-only in .NET.
        ('B' | 'D' | 'X', None) => Err(FormatSpecError::Invalid(spec.to_owned())),
        // `R` *is* `G` of the same case: .NET rewrites the specifier as
        // `(char)(format - ('R' - 'G'))`, so `R` behaves like `G` (upper-case
        // `E` exponent) and `r` like `g` (lower-case `e`). Only a float drops
        // the precision, asking instead for the shortest round-trippable
        // form, which is what `G` without a precision produces; an integer
        // keeps it and formats exactly like `G<precision>`.
        ('R', _) => {
            let general = if fmt == 'R' { 'G' } else { 'g' };
            let precision = match n {
                Number::Float(_) => None,
                Number::Int(_) | Number::UInt(_) => precision,
            };
            Ok(format_buffered(n, general, 'G', precision, info))
        }
        (upper, _) => Ok(format_buffered(n, fmt, upper, precision, info)),
    }
}

/// Splits a standard spec into its letter and optional precision, mirroring
/// `Number.ParseFormatSpecifier`.
///
/// A spec that is not shaped like a standard specifier — one ASCII letter
/// followed by nothing but ASCII digits — is a *custom* pattern, which .NET
/// renders and this port rejects as [`FormatSpecError::Unsupported`]. A spec
/// that has the shape but names no specifier, or asks for more than
/// 999,999,999 digits, is what .NET itself rejects, so it comes back as
/// [`FormatSpecError::Invalid`].
fn parse_spec(spec: &str) -> Result<(char, Option<u32>), FormatSpecError> {
    let mut chars = spec.chars();
    let Some(letter) = chars.next() else {
        return Ok(('G', None));
    };
    let digits = &spec.as_bytes()[1..];
    if !letter.is_ascii_alphabetic() || !digits.iter().all(u8::is_ascii_digit) {
        return Err(FormatSpecError::Unsupported(spec.to_owned()));
    }

    let mut precision: u32 = 0;
    for &b in digits {
        if precision >= 100_000_000 {
            return Err(FormatSpecError::Invalid(spec.to_owned()));
        }
        precision = precision * 10 + u32::from(b - b'0');
    }

    if !matches!(
        letter.to_ascii_uppercase(),
        'B' | 'C' | 'D' | 'E' | 'F' | 'G' | 'N' | 'P' | 'R' | 'X'
    ) {
        return Err(FormatSpecError::Invalid(spec.to_owned()));
    }
    Ok((letter, (!digits.is_empty()).then_some(precision)))
}

const DEFAULT_EXPONENTIAL_PRECISION: u32 = 6;
/// Digits needed to round-trip an `f64`; `G` without a precision stays in
/// fixed-point notation up to this many integral digits.
const MAX_ROUND_TRIP_DIGITS: usize = 17;
const POS_CURRENCY_PATTERNS: [&str; 4] = ["$#", "#$", "$ #", "# $"];
const NEG_CURRENCY_PATTERNS: [&str; 17] = [
    "($#)", "-$#", "$-#", "$#-", "(#$)", "-#$", "#-$", "#$-", "-# $", "-$ #", "# $-", "$ #-",
    "$ -#", "#- $", "($ #)", "(# $)", "$- #",
];
const POS_PERCENT_PATTERNS: [&str; 4] = ["# %", "#%", "%#", "% #"];
const NEG_PERCENT_PATTERNS: [&str; 12] = [
    "-# %", "-#%", "-%#", "%-#", "%#-", "#-%", "#%-", "-% #", "# %-", "% #-", "% -#", "#- %",
];
const NEG_NUMBER_PATTERNS: [&str; 5] = ["(#)", "-#", "- #", "#-", "# -"];

fn pattern_at(patterns: &[&'static str], index: u8) -> &'static str {
    patterns
        .get(usize::from(index))
        .copied()
        .unwrap_or(patterns[0])
}

/// ASCII digits, which every digit sequence here is by construction.
fn ascii(digits: &[u8]) -> &str {
    std::str::from_utf8(digits).expect("digits are ASCII")
}

/// Appends `count` zeroes, in blocks rather than one `char` at a time — a
/// precision of nine digits is legal, so this run can be long.
fn push_zeros(out: &mut String, count: usize) {
    const ZEROS: &str = "00000000000000000000000000000000";
    let mut left = count;
    while left > ZEROS.len() {
        out.push_str(ZEROS);
        left -= ZEROS.len();
    }
    out.push_str(&ZEROS[..left]);
}

/// Writes the decimal digits of `v` into the *end* of `dst`, returning where
/// they start. `dst` needs room for 20 digits.
fn write_digits_u64(dst: &mut [u8], mut v: u64) -> usize {
    let mut at = dst.len();
    loop {
        at -= 1;
        dst[at] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            return at;
        }
    }
}

/// Fills all of `dst` with the decimal digits of `v`, zero-padded on the left.
fn write_padded_u64(dst: &mut [u8], mut v: u64) {
    for slot in dst.iter_mut().rev() {
        *slot = b'0' + (v % 10) as u8;
        v /= 10;
    }
}

/// Writes the decimal digits of `v` into the *end* of `dst`, returning where
/// they start. `dst` needs room for 39 digits.
fn write_digits_u128(dst: &mut [u8], mut v: u128) -> usize {
    /// The largest power of ten a `u64` holds, so each step peels off a whole
    /// chunk and the expensive 128-bit division runs at most twice.
    const CHUNK: u128 = 10_000_000_000_000_000_000;
    let mut at = dst.len();
    while v > u128::from(u64::MAX) {
        at -= 19;
        write_padded_u64(&mut dst[at..at + 19], (v % CHUNK) as u64);
        v /= CHUNK;
    }
    write_digits_u64(&mut dst[..at], v as u64)
}

/// `value.ToString("D<n>")`: magnitude zero-padded to `min_digits`, sign in
/// front.
fn int_to_dec_str(magnitude: u64, negative: bool, min_digits: u32, negative_sign: &str) -> String {
    let mut scratch = [0u8; 20];
    let start = write_digits_u64(&mut scratch, magnitude);
    let digits = &scratch[start..];
    let pad = (min_digits as usize).saturating_sub(digits.len());
    let sign = if negative { negative_sign } else { "" };
    let mut out = String::with_capacity(sign.len() + pad + digits.len());
    out.push_str(sign);
    push_zeros(&mut out, pad);
    out.push_str(ascii(digits));
    out
}

/// `value.ToString("X<n>")` / `("B<n>")`: the two's-complement bit pattern,
/// so a negative value always spans the full 64 bits of its `Value::Int`.
fn int_to_radix_str(
    magnitude: u64,
    negative: bool,
    radix: u32,
    min_digits: u32,
    upper: bool,
) -> String {
    let mut bits = if negative {
        (magnitude as i64).wrapping_neg() as u64
    } else {
        magnitude
    };
    let alphabet: &[u8; 16] = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    debug_assert!(radix == 16 || radix == 2, "only hex and binary have a spec");
    let shift = radix.trailing_zeros();
    let mask = u64::from(radix - 1);
    let mut scratch = [0u8; 64];
    let mut at = scratch.len();
    loop {
        at -= 1;
        scratch[at] = alphabet[(bits & mask) as usize];
        bits >>= shift;
        if bits == 0 {
            break;
        }
    }
    let body = &scratch[at..];
    let pad = (min_digits as usize).saturating_sub(body.len());
    let mut out = String::with_capacity(pad + body.len());
    push_zeros(&mut out, pad);
    out.push_str(ascii(body));
    out
}

/// Room for every digit sequence a template is likely to produce: 20 digits
/// for a `u64` magnitude, 39 for the widest expansion the 128-bit path below
/// reaches, and enough beyond that to cover the exact expansion of a small
/// fraction — `0.1` alone spans 56 digits. Only a value with a large binary
/// exponent, whose expansion runs to hundreds of digits, spills to the heap.
const INLINE_DIGITS: usize = 64;

/// A digit sequence, ASCII, most significant first.
enum Digits {
    Inline {
        buf: [u8; INLINE_DIGITS],
        len: usize,
    },
    Heap(Vec<u8>),
}

impl Digits {
    fn empty() -> Self {
        Digits::Inline {
            buf: [0; INLINE_DIGITS],
            len: 0,
        }
    }

    /// The digits of `scratch[start..]`, moved to the front of an inline
    /// buffer — the writers above fill from the right.
    fn from_tail(scratch: &[u8; INLINE_DIGITS], start: usize) -> Self {
        let mut buf = *scratch;
        buf.copy_within(start.., 0);
        Digits::Inline {
            buf,
            len: INLINE_DIGITS - start,
        }
    }

    fn as_slice(&self) -> &[u8] {
        match self {
            Digits::Inline { buf, len } => &buf[..*len],
            Digits::Heap(digits) => digits,
        }
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        match self {
            Digits::Inline { buf, len } => &mut buf[..*len],
            Digits::Heap(digits) => digits,
        }
    }

    fn len(&self) -> usize {
        self.as_slice().len()
    }

    fn truncate(&mut self, len: usize) {
        match self {
            Digits::Inline { len: current, .. } => *current = (*current).min(len),
            Digits::Heap(digits) => digits.truncate(len),
        }
    }

    fn pop_trailing_zeros(&mut self) {
        let keep = self.as_slice().iter().rposition(|&d| d != b'0');
        self.truncate(keep.map_or(0, |last| last + 1));
    }
}

/// A value split into decimal digits and a scale, mirroring .NET's
/// `NumberBuffer`: the value is `0.<digits> * 10^scale`, and `digits` is empty
/// for zero.
struct NumberBuffer {
    digits: Digits,
    scale: i32,
    negative: bool,
    is_float: bool,
}

fn format_buffered(
    n: Number,
    fmt: char,
    upper: char,
    precision: Option<u32>,
    info: &NumberFormat,
) -> String {
    let shortest = upper == 'G' && precision.unwrap_or(0) == 0;

    let mut buf = match (n, n.integer()) {
        // .NET's integer fast path: `G` and `G0` bypass the buffer entirely,
        // so an integer never switches to scientific notation there.
        (_, Some((magnitude, negative))) if shortest => {
            return int_to_dec_str(magnitude, negative, 0, info.negative_sign)
        }
        (_, Some((magnitude, negative))) => int_buffer(magnitude, negative),
        (Number::Float(v), None) if shortest => shortest_float_buffer(v),
        (Number::Float(v), None) => exact_float_buffer(v),
        (_, None) => unreachable!("a non-float always has an integer form"),
    };

    // One allocation covers every rendering that is not a wall of digits.
    let mut out = String::with_capacity(32);
    match upper {
        'C' => {
            let decimals = precision.unwrap_or(u32::from(info.currency_decimal_digits)) as i32;
            let pos = buf.scale + decimals;
            round(&mut buf, pos);
            format_currency(&mut out, &buf, decimals, info);
        }
        'F' => {
            let decimals = precision.unwrap_or(u32::from(info.number_decimal_digits)) as i32;
            let pos = buf.scale + decimals;
            round(&mut buf, pos);
            if buf.negative {
                out.push_str(info.negative_sign);
            }
            format_fixed(&mut out, &buf, decimals, None, info.decimal_separator, "");
        }
        'N' => {
            let decimals = precision.unwrap_or(u32::from(info.number_decimal_digits)) as i32;
            let pos = buf.scale + decimals;
            round(&mut buf, pos);
            format_grouped(&mut out, &buf, decimals, info);
        }
        'P' => {
            let decimals = precision.unwrap_or(u32::from(info.percent_decimal_digits)) as i32;
            buf.scale += 2;
            let pos = buf.scale + decimals;
            round(&mut buf, pos);
            format_percent(&mut out, &buf, decimals, info);
        }
        'E' => {
            let significant = precision.unwrap_or(DEFAULT_EXPONENTIAL_PRECISION) as i32 + 1;
            round(&mut buf, significant);
            if buf.negative {
                out.push_str(info.negative_sign);
            }
            format_scientific(&mut out, &buf, significant, info, fmt);
        }
        'G' => {
            let significant = match precision {
                Some(p) if p >= 1 => p as i32,
                _ => buf.digits.len().max(MAX_ROUND_TRIP_DIGITS) as i32,
            };
            round(&mut buf, significant);
            if buf.negative {
                out.push_str(info.negative_sign);
            }
            let exp_char = if fmt == 'G' { 'E' } else { 'e' };
            format_general(&mut out, &buf, significant, info, exp_char);
        }
        _ => unreachable!("parse_spec accepts no other specifier"),
    }
    out
}

fn int_buffer(magnitude: u64, negative: bool) -> NumberBuffer {
    let digits = if magnitude == 0 {
        Digits::empty()
    } else {
        let mut scratch = [0u8; INLINE_DIGITS];
        let start = write_digits_u64(&mut scratch, magnitude);
        Digits::from_tail(&scratch, start)
    };
    NumberBuffer {
        scale: digits.len() as i32,
        digits,
        negative,
        is_float: false,
    }
}

/// A short ASCII sink, so the two places that need `core::fmt` do not have to
/// allocate a `String` to read it back.
struct Scratch {
    buf: [u8; 48],
    len: usize,
}

impl Scratch {
    fn new() -> Self {
        Scratch {
            buf: [0; 48],
            len: 0,
        }
    }

    fn as_str(&self) -> &str {
        ascii(&self.buf[..self.len])
    }
}

impl std::fmt::Write for Scratch {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        let end = self.len + text.len();
        // `LowerExp` on an `f64` never exceeds 24 bytes, and neither does the
        // round-trip probe, so this is unreachable.
        let room = self.buf.get_mut(self.len..end).ok_or(std::fmt::Error)?;
        room.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// The shortest digits that round-trip back to `v`, which is what `G` without
/// a precision asks for. Rust's `LowerExp` produces exactly those.
fn shortest_float_buffer(v: f64) -> NumberBuffer {
    if v == 0.0 {
        return NumberBuffer {
            digits: Digits::empty(),
            scale: 0,
            negative: v.is_sign_negative(),
            is_float: true,
        };
    }
    use std::fmt::Write;
    let mut repr = Scratch::new();
    write!(repr, "{:e}", v.abs()).expect("LowerExp on a finite f64 is short");
    let (mantissa, exponent) = repr
        .as_str()
        .split_once('e')
        .expect("LowerExp emits an exponent");
    let mut scratch = [0u8; INLINE_DIGITS];
    let mut len = 0;
    for digit in mantissa.bytes().filter(u8::is_ascii_digit) {
        scratch[len] = digit;
        len += 1;
    }
    let exponent: i32 = exponent.parse().expect("LowerExp emits a decimal exponent");
    let mut buf = NumberBuffer {
        digits: Digits::Inline { buf: scratch, len },
        scale: exponent + 1,
        negative: v.is_sign_negative(),
        is_float: true,
    };

    // Where two equally short representations are equidistant from `v`, Rust
    // takes the larger and .NET the even one.
    if may_land_on_midpoint(v) {
        let mut even = exact_float_buffer(v);
        let len = buf.digits.len();
        if even.digits.len() == len + 1 && even.digits.as_slice()[len] == b'5' {
            round(&mut even, len as i32);
            if even.digits.as_slice() != buf.digits.as_slice() && round_trips(&even, v) {
                buf = even;
            }
        }
    }
    buf
}

/// Whether rounding `v` to its shortest round-trip length can land exactly on a
/// midpoint. That needs an exact expansion of at most 18 digits, which rules
/// out integral values (their expansion never ends in a lone `5` within half an
/// ulp of a shorter form) and anything needing more than 5^30, since 5^31
/// alone already spans 22 digits.
fn may_land_on_midpoint(v: f64) -> bool {
    let bits = v.to_bits();
    let biased_exponent = ((bits >> 52) & 0x7ff) as i32;
    if biased_exponent == 0 {
        return false;
    }
    let exponent = biased_exponent - 1075;
    if exponent >= 0 {
        return false;
    }
    let mantissa = (bits & ((1u64 << 52) - 1)) | (1u64 << 52);
    let shift = mantissa.trailing_zeros().min(exponent.unsigned_abs());
    exponent.unsigned_abs() - shift <= 30
}

fn round_trips(buf: &NumberBuffer, v: f64) -> bool {
    use std::fmt::Write;
    let digits = ascii(buf.digits.as_slice());
    let exponent = buf.scale - buf.digits.len() as i32;
    let mut probe = Scratch::new();
    write!(probe, "{digits}e{exponent}").expect("a rounded shortest form is short");
    probe
        .as_str()
        .parse::<f64>()
        .is_ok_and(|parsed| parsed == v.abs())
}

/// The full decimal expansion of `v`: an `f64` is `mantissa * 2^exponent`, and
/// `2^-k` is `5^k / 10^k`, so the expansion is always finite.
fn exact_float_buffer(v: f64) -> NumberBuffer {
    let bits = v.to_bits();
    let negative = v.is_sign_negative();
    let biased_exponent = ((bits >> 52) & 0x7ff) as i32;
    let raw_mantissa = bits & ((1u64 << 52) - 1);
    let (mut mantissa, mut exponent) = if biased_exponent == 0 {
        (raw_mantissa, -1074)
    } else {
        (raw_mantissa | (1u64 << 52), biased_exponent - 1075)
    };

    if mantissa == 0 {
        return NumberBuffer {
            digits: Digits::empty(),
            scale: 0,
            negative,
            is_float: true,
        };
    }

    // The trailing zero bits of the mantissa cancel against the negative
    // exponent — `m * 2^-k` is `(m >> s) * 2^-(k - s)` — which is the whole
    // difference between the 42 fraction digits `1234.5` asks for as raw
    // IEEE fields and the single one it actually has. The expansion is the
    // same either way, so this only decides how much work computing it takes.
    if exponent < 0 {
        let shift = mantissa.trailing_zeros().min(exponent.unsigned_abs());
        mantissa >>= shift;
        exponent += shift as i32;
    }

    short_float_buffer(mantissa, exponent, negative)
        .unwrap_or_else(|| big_float_buffer(mantissa, exponent, negative))
}

/// The exact expansion of `mantissa * 2^exponent` for any exponent, at the
/// cost of big-integer arithmetic and a heap buffer.
fn big_float_buffer(mantissa: u64, exponent: i32, negative: bool) -> NumberBuffer {
    let mut value = BigDecimal::new(mantissa);
    let fraction_digits = if exponent >= 0 {
        value.mul_pow2(exponent as u32);
        0
    } else {
        value.mul_pow5(exponent.unsigned_abs());
        exponent.unsigned_abs() as i32
    };

    let mut digits = value.digits();
    let scale = digits.len() as i32 - fraction_digits;
    digits.pop_trailing_zeros();
    NumberBuffer {
        digits,
        scale,
        negative,
        is_float: true,
    }
}

/// `5^k` for every `k` whose power still fits a `u128`; `5^56` does not.
const POW5: [u128; 56] = {
    let mut table = [1u128; 56];
    let mut k = 1;
    while k < 56 {
        table[k] = table[k - 1] * 5;
        k += 1;
    }
    table
};

/// The exact expansion of `mantissa * 2^exponent` when it fits 128 bits, which
/// covers every value whose binary exponent is small once the mantissa's
/// trailing zeros are cancelled — that is, essentially every number a template
/// formats. `None` asks for the big-integer path.
fn short_float_buffer(mantissa: u64, exponent: i32, negative: bool) -> Option<NumberBuffer> {
    let mantissa = u128::from(mantissa);
    let (value, fraction_digits) = if exponent >= 0 {
        let shift = exponent as u32;
        if shift > mantissa.leading_zeros() {
            return None;
        }
        (mantissa << shift, 0)
    } else {
        let power = POW5.get(exponent.unsigned_abs() as usize)?;
        (
            mantissa.checked_mul(*power)?,
            exponent.unsigned_abs() as i32,
        )
    };

    let mut scratch = [0u8; INLINE_DIGITS];
    let start = write_digits_u128(&mut scratch, value);
    let mut digits = Digits::from_tail(&scratch, start);
    let scale = digits.len() as i32 - fraction_digits;
    digits.pop_trailing_zeros();
    Some(NumberBuffer {
        digits,
        scale,
        negative,
        is_float: true,
    })
}

/// A non-negative integer in base 10^9, least significant limb first. The
/// widest value that ever lands here is `2^52 * 5^1074` — 767 digits, or 86
/// limbs — so the limbs live in a fixed array and the expansion costs no
/// allocation at all.
struct BigDecimal {
    limbs: [u32; MAX_LIMBS],
    len: usize,
}

const LIMB_BASE: u64 = 1_000_000_000;
const MAX_LIMBS: usize = 87;

impl BigDecimal {
    fn new(mut v: u64) -> Self {
        let mut value = BigDecimal {
            limbs: [0; MAX_LIMBS],
            len: 0,
        };
        while v > 0 {
            value.limbs[value.len] = (v % LIMB_BASE) as u32;
            value.len += 1;
            v /= LIMB_BASE;
        }
        value
    }

    fn mul_small(&mut self, factor: u32) {
        let mut carry: u64 = 0;
        for limb in &mut self.limbs[..self.len] {
            let product = u64::from(*limb) * u64::from(factor) + carry;
            *limb = (product % LIMB_BASE) as u32;
            carry = product / LIMB_BASE;
        }
        while carry > 0 {
            self.limbs[self.len] = (carry % LIMB_BASE) as u32;
            self.len += 1;
            carry /= LIMB_BASE;
        }
    }

    /// Steps of 2^29 keep `limb * factor + carry` inside a `u64`.
    fn mul_pow2(&mut self, mut exponent: u32) {
        while exponent > 0 {
            let step = exponent.min(29);
            self.mul_small(1 << step);
            exponent -= step;
        }
    }

    /// Steps of 5^13 keep `limb * factor + carry` inside a `u64`.
    fn mul_pow5(&mut self, mut exponent: u32) {
        while exponent > 0 {
            let step = exponent.min(13);
            self.mul_small(5u32.pow(step));
            exponent -= step;
        }
    }

    fn digits(&self) -> Digits {
        let Some((&top, rest)) = self.limbs[..self.len].split_last() else {
            return Digits::empty();
        };
        let mut head = [0u8; 10];
        let start = write_digits_u64(&mut head, u64::from(top));
        let head = &head[start..];
        let total = head.len() + rest.len() * 9;

        // Nine digits per limb after the first, most significant limb first.
        let mut digits = if total <= INLINE_DIGITS {
            Digits::Inline {
                buf: [0; INLINE_DIGITS],
                len: total,
            }
        } else {
            Digits::Heap(vec![0; total])
        };
        let out = digits.as_mut_slice();
        out[..head.len()].copy_from_slice(head);
        let mut at = head.len();
        for &limb in rest.iter().rev() {
            write_padded_u64(&mut out[at..at + 9], u64::from(limb));
            at += 9;
        }
        digits
    }
}

/// Keeps the leading `pos` digits, rounding the dropped tail in. Mirrors
/// `Number.RoundNumber`, including its normalization of a zero result.
fn round(buf: &mut NumberBuffer, pos: i32) {
    let mut kept = pos.clamp(0, buf.digits.len() as i32) as usize;

    if pos >= 0 && should_round_up(buf, pos as usize) {
        let digits = buf.digits.as_mut_slice();
        while kept > 0 && digits[kept - 1] == b'9' {
            kept -= 1;
        }
        if kept > 0 {
            digits[kept - 1] += 1;
        } else {
            buf.scale += 1;
            buf.digits.as_mut_slice()[0] = b'1';
            kept = 1;
        }
    } else {
        let digits = buf.digits.as_slice();
        while kept > 0 && digits[kept - 1] == b'0' {
            kept -= 1;
        }
    }

    buf.digits.truncate(kept);
    if kept == 0 {
        // Integers have no negative zero, but `-0.0` keeps its sign.
        if !buf.is_float {
            buf.negative = false;
        }
        buf.scale = 0;
    }
}

fn should_round_up(buf: &NumberBuffer, pos: usize) -> bool {
    let digits = buf.digits.as_slice();
    let Some(&digit) = digits.get(pos) else {
        return false;
    };
    if !buf.is_float {
        return digit >= b'5';
    }
    match digit.cmp(&b'5') {
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Greater => true,
        // The digits are exact, so a bare trailing `5` is a true midpoint and
        // IEEE rounding takes it to the even neighbour.
        std::cmp::Ordering::Equal => {
            digits[pos + 1..].iter().any(|&d| d != b'0')
                || (pos > 0 && (digits[pos - 1] - b'0') % 2 == 1)
        }
    }
}

/// How many groups the integral digits split into and how many digits the
/// leading one takes, per .NET `NumberGroupSizes`: the sizes apply right to
/// left, the last one repeats, and a `0` size ends grouping. Counting the
/// groups instead of listing them keeps this allocation-free; emitting them
/// left to right then walks the sizes backwards.
fn group_layout(int_len: usize, sizes: &[u8]) -> (usize, usize) {
    let mut remaining = int_len;
    let mut index = 0;
    let mut groups = 1;
    loop {
        let size = usize::from(sizes[index]);
        if size == 0 || size >= remaining {
            return (groups, remaining);
        }
        remaining -= size;
        groups += 1;
        if index + 1 < sizes.len() {
            index += 1;
        }
    }
}

/// Appends `count` digits starting at `from`, treating anything past the end
/// of the sequence as a zero.
fn push_digits(out: &mut String, digits: &[u8], from: usize, count: usize) {
    let from = from.min(digits.len());
    let available = (digits.len() - from).min(count);
    out.push_str(ascii(&digits[from..from + available]));
    push_zeros(out, count - available);
}

fn format_fixed(
    out: &mut String,
    buf: &NumberBuffer,
    mut n_max_digits: i32,
    group_sizes: Option<&[u8]>,
    s_decimal: &str,
    s_group: &str,
) {
    let digits = buf.digits.as_slice();
    let int_len = buf.scale.max(0) as usize;
    let next = int_len.min(digits.len());

    match group_sizes {
        _ if int_len == 0 => out.push('0'),
        Some(sizes) if !sizes.is_empty() && sizes[0] != 0 => {
            let (groups, leading) = group_layout(int_len, sizes);
            push_digits(out, digits, 0, leading);
            let mut emitted = leading;
            for group in (0..groups - 1).rev() {
                out.push_str(s_group);
                let size = usize::from(sizes[group.min(sizes.len() - 1)]);
                push_digits(out, digits, emitted, size);
                emitted += size;
            }
        }
        _ => push_digits(out, digits, 0, int_len),
    }

    if n_max_digits > 0 {
        out.push_str(s_decimal);
        if buf.scale < 0 {
            let zeroes = (-buf.scale).min(n_max_digits);
            push_zeros(out, zeroes as usize);
            n_max_digits -= zeroes;
        }
        push_digits(out, digits, next, n_max_digits as usize);
    }
}

fn format_currency(out: &mut String, buf: &NumberBuffer, n_max_digits: i32, info: &NumberFormat) {
    let pattern = if buf.negative {
        pattern_at(&NEG_CURRENCY_PATTERNS, info.currency_negative_pattern)
    } else {
        pattern_at(&POS_CURRENCY_PATTERNS, info.currency_positive_pattern)
    };
    for ch in pattern.chars() {
        match ch {
            '#' => format_fixed(
                out,
                buf,
                n_max_digits,
                Some(info.currency_group_sizes),
                info.currency_decimal_separator,
                info.currency_group_separator,
            ),
            '-' => out.push_str(info.negative_sign),
            '$' => out.push_str(info.currency_symbol),
            _ => out.push(ch),
        }
    }
}

fn format_percent(out: &mut String, buf: &NumberBuffer, n_max_digits: i32, info: &NumberFormat) {
    let pattern = if buf.negative {
        pattern_at(&NEG_PERCENT_PATTERNS, info.percent_negative_pattern)
    } else {
        pattern_at(&POS_PERCENT_PATTERNS, info.percent_positive_pattern)
    };
    for ch in pattern.chars() {
        match ch {
            '#' => format_fixed(
                out,
                buf,
                n_max_digits,
                Some(info.percent_group_sizes),
                info.percent_decimal_separator,
                info.percent_group_separator,
            ),
            '-' => out.push_str(info.negative_sign),
            '%' => out.push_str(info.percent_symbol),
            _ => out.push(ch),
        }
    }
}

fn format_grouped(out: &mut String, buf: &NumberBuffer, n_max_digits: i32, info: &NumberFormat) {
    let pattern = if buf.negative {
        pattern_at(&NEG_NUMBER_PATTERNS, info.number_negative_pattern)
    } else {
        "#"
    };
    for ch in pattern.chars() {
        match ch {
            '#' => format_fixed(
                out,
                buf,
                n_max_digits,
                Some(info.group_sizes),
                info.decimal_separator,
                info.group_separator,
            ),
            '-' => out.push_str(info.negative_sign),
            _ => out.push(ch),
        }
    }
}

fn format_scientific(
    out: &mut String,
    buf: &NumberBuffer,
    n_max_digits: i32,
    info: &NumberFormat,
    exp_char: char,
) {
    let digits = buf.digits.as_slice();
    push_digits(out, digits, 0, 1);
    if n_max_digits != 1 {
        out.push_str(info.decimal_separator);
    }
    push_digits(out, digits, 1, (n_max_digits - 1).max(0) as usize);
    let exponent = if digits.is_empty() { 0 } else { buf.scale - 1 };
    format_exponent(out, info, exponent, exp_char, 3);
}

fn format_general(
    out: &mut String,
    buf: &NumberBuffer,
    n_max_digits: i32,
    info: &NumberFormat,
    exp_char: char,
) {
    let digits = buf.digits.as_slice();
    let mut dig_pos = buf.scale;
    let scientific = dig_pos > n_max_digits || dig_pos < -3;
    if scientific {
        dig_pos = 1;
    }

    let mut next = 0;
    if dig_pos > 0 {
        next = digits.len().min(dig_pos as usize);
        push_digits(out, digits, 0, dig_pos as usize);
    } else {
        out.push('0');
    }

    if next < digits.len() || dig_pos < 0 {
        out.push_str(info.decimal_separator);
        push_zeros(out, dig_pos.min(0).unsigned_abs() as usize);
        out.push_str(ascii(&digits[next..]));
    }

    if scientific {
        format_exponent(out, info, buf.scale - 1, exp_char, 2);
    }
}

fn format_exponent(
    out: &mut String,
    info: &NumberFormat,
    value: i32,
    exp_char: char,
    min_digits: usize,
) {
    out.push(exp_char);
    if value < 0 {
        out.push_str(info.negative_sign);
    } else {
        out.push_str(info.positive_sign);
    }
    let mut scratch = [0u8; 20];
    let start = write_digits_u64(&mut scratch, u64::from(value.unsigned_abs()));
    let digits = &scratch[start..];
    push_zeros(out, min_digits.saturating_sub(digits.len()));
    out.push_str(ascii(digits));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fmt::culture::invariant;

    fn int(v: i64, spec: &str) -> String {
        format_number(Number::Int(v), spec, invariant()).expect("standard spec")
    }

    fn float(v: f64, spec: &str) -> String {
        format_number(Number::Float(v), spec, invariant()).expect("standard spec")
    }

    /// The limb array is sized for the widest expansion any `f64` has, which
    /// belongs to the largest denormal: an odd 52-bit mantissa over 5^1074.
    /// Running short of limbs would panic, so pin the count.
    #[test]
    fn the_widest_expansion_fits_the_limb_array() {
        let widest = f64::from_bits((1u64 << 52) - 1);
        let buf = exact_float_buffer(widest);
        assert_eq!(buf.digits.len(), 767);
        assert_eq!(buf.scale, -307);
        assert_eq!(float(widest, "E5"), "2.22507E-308");
        // The other end: the widest integral expansion, from `f64::MAX`.
        assert_eq!(exact_float_buffer(f64::MAX).digits.len(), 309);
    }

    /// The 128-bit fast path and the big-integer path have to agree digit for
    /// digit, since which one runs is invisible in the output.
    #[test]
    fn the_short_expansion_agrees_with_the_big_integer_one() {
        // A xorshift walk over the bit patterns, so the sample covers every
        // exponent range rather than the values a literal would suggest.
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut checked = 0;
        for _ in 0..200_000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let v = f64::from_bits(state);
            if !v.is_finite() || v == 0.0 {
                continue;
            }
            let bits = v.to_bits();
            let biased = ((bits >> 52) & 0x7ff) as i32;
            let raw = bits & ((1u64 << 52) - 1);
            let (mut mantissa, mut exponent) = if biased == 0 {
                (raw, -1074)
            } else {
                (raw | (1u64 << 52), biased - 1075)
            };
            if mantissa == 0 {
                continue;
            }
            if exponent < 0 {
                let shift = mantissa.trailing_zeros().min(exponent.unsigned_abs());
                mantissa >>= shift;
                exponent += shift as i32;
            }
            let Some(short) = short_float_buffer(mantissa, exponent, false) else {
                continue;
            };
            let big = big_float_buffer(mantissa, exponent, false);
            assert_eq!(
                ascii(short.digits.as_slice()),
                ascii(big.digits.as_slice()),
                "digits of {v:e}"
            );
            assert_eq!(short.scale, big.scale, "scale of {v:e}");
            checked += 1;
        }
        assert!(checked > 1000, "only {checked} values took the fast path");

        // And directly over the fast path's own boundary: every exponent it
        // accepts, against mantissas that straddle the 128-bit limit.
        for exponent in -60i32..80 {
            for mantissa in [
                1u64,
                3,
                12345,
                (1 << 52) + 1,
                (1 << 53) - 1,
                u64::from(u32::MAX),
            ] {
                let Some(short) = short_float_buffer(mantissa, exponent, false) else {
                    continue;
                };
                let big = big_float_buffer(mantissa, exponent, false);
                assert_eq!(
                    ascii(short.digits.as_slice()),
                    ascii(big.digits.as_slice()),
                    "digits of {mantissa} * 2^{exponent}"
                );
                assert_eq!(short.scale, big.scale, "scale of {mantissa} * 2^{exponent}");
            }
        }
    }

    #[test]
    fn general_specifier_matches_shortest_round_trip() {
        assert_eq!(float(0.0, ""), "0");
        assert_eq!(float(0.0, "G"), "0");
        assert_eq!(float(0.0, "g"), "0");
        assert_eq!(float(1.0, ""), "1");
        assert_eq!(float(1.0, "G"), "1");
        assert_eq!(float(1.0, "g"), "1");
        assert_eq!(float(-123.456, ""), "-123.456");
        assert_eq!(float(-123.456, "G"), "-123.456");
        assert_eq!(float(-123.456, "g"), "-123.456");
        assert_eq!(float(0.1, ""), "0.1");
        assert_eq!(float(0.1, "G"), "0.1");
        assert_eq!(float(0.1, "g"), "0.1");
        assert_eq!(float(1234.5678, ""), "1234.5678");
        assert_eq!(float(1234.5678, "G"), "1234.5678");
        assert_eq!(float(1234.5678, "g"), "1234.5678");
        assert_eq!(float(0.30000000000000004, ""), "0.30000000000000004");
        assert_eq!(float(0.30000000000000004, "G"), "0.30000000000000004");
        assert_eq!(float(0.30000000000000004, "g"), "0.30000000000000004");
        assert_eq!(float(1.0 / 3.0, ""), "0.3333333333333333");
        assert_eq!(float(1.0 / 3.0, "G"), "0.3333333333333333");
        assert_eq!(float(1.0 / 3.0, "g"), "0.3333333333333333");
        assert_eq!(float(-1.23456789e-25, ""), "-1.23456789E-25");
        assert_eq!(float(-1.23456789e-25, "G"), "-1.23456789E-25");
        assert_eq!(float(-1.23456789e-25, "g"), "-1.23456789e-25");
    }

    #[test]
    fn general_specifier_switches_to_scientific_by_scale() {
        assert_eq!(float(1e14, "G"), "100000000000000");
        assert_eq!(float(1e15, "G"), "1000000000000000");
        assert_eq!(float(1e16, "G"), "10000000000000000");
        assert_eq!(float(1e17, "G"), "1E+17");
        assert_eq!(float(1e-3, "G"), "0.001");
        assert_eq!(float(1e-4, "G"), "0.0001");
        assert_eq!(float(1e-5, "G"), "1E-05");
        assert_eq!(float(1e21, "G"), "1E+21");
    }

    #[test]
    fn general_specifier_with_precision() {
        assert_eq!(float(123.4546, "G1"), "1E+02");
        assert_eq!(float(123.4546, "G2"), "1.2E+02");
        assert_eq!(float(123.4546, "G3"), "123");
        assert_eq!(float(123.4546, "G4"), "123.5");
        assert_eq!(float(123.4546, "G17"), "123.4546");
        assert_eq!(float(123.4546, "G20"), "123.45459999999999923");
        assert_eq!(float(99.99, "G1"), "1E+02");
        assert_eq!(float(99.99, "G2"), "1E+02");
        assert_eq!(float(99.99, "G3"), "100");
        assert_eq!(float(99.99, "G4"), "99.99");
        assert_eq!(float(99.99, "G17"), "99.989999999999995");
        assert_eq!(float(99.99, "G20"), "99.989999999999994884");
        assert_eq!(float(0.000001234, "G1"), "1E-06");
        assert_eq!(float(0.000001234, "G2"), "1.2E-06");
        assert_eq!(float(0.000001234, "G3"), "1.23E-06");
        assert_eq!(float(0.000001234, "G4"), "1.234E-06");
        assert_eq!(float(0.000001234, "G17"), "1.234E-06");
        assert_eq!(float(0.000001234, "G20"), "1.233999999999999959E-06");
        assert_eq!(float(1e17, "G1"), "1E+17");
        assert_eq!(float(1e17, "G2"), "1E+17");
        assert_eq!(float(1e17, "G3"), "1E+17");
        assert_eq!(float(1e17, "G4"), "1E+17");
        assert_eq!(float(1e17, "G17"), "1E+17");
        assert_eq!(float(1e17, "G20"), "100000000000000000");
        assert_eq!(float(0.1, "G1"), "0.1");
        assert_eq!(float(0.1, "G2"), "0.1");
        assert_eq!(float(0.1, "G3"), "0.1");
        assert_eq!(float(0.1, "G4"), "0.1");
        assert_eq!(float(0.1, "G17"), "0.10000000000000001");
        assert_eq!(float(0.1, "G20"), "0.10000000000000000555");
    }

    #[test]
    fn general_specifier_on_integers() {
        assert_eq!(int(0, ""), "0");
        assert_eq!(int(0, "G"), "0");
        assert_eq!(int(0, "G0"), "0");
        assert_eq!(int(0, "G5"), "0");
        assert_eq!(int(0, "G20"), "0");
        assert_eq!(int(0, "g5"), "0");
        assert_eq!(int(-1234, ""), "-1234");
        assert_eq!(int(-1234, "G"), "-1234");
        assert_eq!(int(-1234, "G0"), "-1234");
        assert_eq!(int(-1234, "G5"), "-1234");
        assert_eq!(int(-1234, "G20"), "-1234");
        assert_eq!(int(-1234, "g5"), "-1234");
        assert_eq!(int(1234567890, ""), "1234567890");
        assert_eq!(int(1234567890, "G"), "1234567890");
        assert_eq!(int(1234567890, "G0"), "1234567890");
        assert_eq!(int(1234567890, "G5"), "1.2346E+09");
        assert_eq!(int(1234567890, "G20"), "1234567890");
        assert_eq!(int(1234567890, "g5"), "1.2346e+09");
        assert_eq!(int(i64::MAX, ""), "9223372036854775807");
        assert_eq!(int(i64::MAX, "G"), "9223372036854775807");
        assert_eq!(int(i64::MAX, "G0"), "9223372036854775807");
        assert_eq!(int(i64::MAX, "G5"), "9.2234E+18");
        assert_eq!(int(i64::MAX, "G20"), "9223372036854775807");
        assert_eq!(int(i64::MAX, "g5"), "9.2234e+18");
        assert_eq!(int(i64::MIN, ""), "-9223372036854775808");
        assert_eq!(int(i64::MIN, "G"), "-9223372036854775808");
        assert_eq!(int(i64::MIN, "G0"), "-9223372036854775808");
        assert_eq!(int(i64::MIN, "G5"), "-9.2234E+18");
        assert_eq!(int(i64::MIN, "G20"), "-9223372036854775808");
        assert_eq!(int(i64::MIN, "g5"), "-9.2234e+18");
    }

    #[test]
    fn fixed_point_specifier() {
        assert_eq!(float(1234.567, "F"), "1234.57");
        assert_eq!(float(1234.567, "F0"), "1235");
        assert_eq!(float(1234.567, "F1"), "1234.6");
        assert_eq!(float(1234.567, "F2"), "1234.57");
        assert_eq!(float(1234.567, "F4"), "1234.5670");
        assert_eq!(float(1234.0, "F"), "1234.00");
        assert_eq!(float(1234.0, "F0"), "1234");
        assert_eq!(float(1234.0, "F1"), "1234.0");
        assert_eq!(float(1234.0, "F2"), "1234.00");
        assert_eq!(float(1234.0, "F4"), "1234.0000");
        assert_eq!(float(-1234.56, "F"), "-1234.56");
        assert_eq!(float(-1234.56, "F0"), "-1235");
        assert_eq!(float(-1234.56, "F1"), "-1234.6");
        assert_eq!(float(-1234.56, "F2"), "-1234.56");
        assert_eq!(float(-1234.56, "F4"), "-1234.5600");
        assert_eq!(float(0.0, "F"), "0.00");
        assert_eq!(float(0.0, "F0"), "0");
        assert_eq!(float(0.0, "F1"), "0.0");
        assert_eq!(float(0.0, "F2"), "0.00");
        assert_eq!(float(0.0, "F4"), "0.0000");
        assert_eq!(float(-0.4, "F"), "-0.40");
        assert_eq!(float(-0.4, "F0"), "-0");
        assert_eq!(float(-0.4, "F1"), "-0.4");
        assert_eq!(float(-0.4, "F2"), "-0.40");
        assert_eq!(float(-0.4, "F4"), "-0.4000");
        assert_eq!(float(0.004, "F"), "0.00");
        assert_eq!(float(0.004, "F0"), "0");
        assert_eq!(float(0.004, "F1"), "0.0");
        assert_eq!(float(0.004, "F2"), "0.00");
        assert_eq!(float(0.004, "F4"), "0.0040");
        assert_eq!(float(0.006, "F"), "0.01");
        assert_eq!(float(0.006, "F0"), "0");
        assert_eq!(float(0.006, "F1"), "0.0");
        assert_eq!(float(0.006, "F2"), "0.01");
        assert_eq!(float(0.006, "F4"), "0.0060");
    }

    #[test]
    fn fixed_point_uses_the_exact_binary_expansion() {
        assert_eq!(float(0.1, "F0"), "0");
        assert_eq!(float(0.1, "F2"), "0.10");
        assert_eq!(float(0.1, "F20"), "0.10000000000000000555");
        assert_eq!(float(0.1, "F30"), "0.100000000000000005551115123126");
        assert_eq!(float(6.02e23, "F0"), "601999999999999995805696");
        assert_eq!(float(6.02e23, "F2"), "601999999999999995805696.00");
        assert_eq!(
            float(6.02e23, "F20"),
            "601999999999999995805696.00000000000000000000"
        );
        assert_eq!(
            float(6.02e23, "F30"),
            "601999999999999995805696.000000000000000000000000000000"
        );
        assert_eq!(float(2.675, "F0"), "3");
        assert_eq!(float(2.675, "F2"), "2.67");
        assert_eq!(float(2.675, "F20"), "2.67499999999999982236");
        assert_eq!(float(2.675, "F30"), "2.674999999999999822364316059975");
        assert_eq!(float(1.005, "F0"), "1");
        assert_eq!(float(1.005, "F2"), "1.00");
        assert_eq!(float(1.005, "F20"), "1.00499999999999989342");
        assert_eq!(float(1.005, "F30"), "1.004999999999999893418589635985");
    }

    #[test]
    fn integers_round_half_away_from_zero() {
        assert_eq!(int(1050, "G1"), "1E+03");
        assert_eq!(int(1050, "G2"), "1.1E+03");
        assert_eq!(int(1050, "G3"), "1.05E+03");
        assert_eq!(int(1050, "E0"), "1E+003");
        assert_eq!(int(1050, "E1"), "1.1E+003");
        assert_eq!(int(1150, "G1"), "1E+03");
        assert_eq!(int(1150, "G2"), "1.2E+03");
        assert_eq!(int(1150, "G3"), "1.15E+03");
        assert_eq!(int(1150, "E0"), "1E+003");
        assert_eq!(int(1150, "E1"), "1.2E+003");
        assert_eq!(int(2500, "G1"), "3E+03");
        assert_eq!(int(2500, "G2"), "2.5E+03");
        assert_eq!(int(2500, "G3"), "2.5E+03");
        assert_eq!(int(2500, "E0"), "3E+003");
        assert_eq!(int(2500, "E1"), "2.5E+003");
        assert_eq!(int(-2500, "G1"), "-3E+03");
        assert_eq!(int(-2500, "G2"), "-2.5E+03");
        assert_eq!(int(-2500, "G3"), "-2.5E+03");
        assert_eq!(int(-2500, "E0"), "-3E+003");
        assert_eq!(int(-2500, "E1"), "-2.5E+003");
        assert_eq!(int(1996, "G1"), "2E+03");
        assert_eq!(int(1996, "G2"), "2E+03");
        assert_eq!(int(1996, "G3"), "2E+03");
        assert_eq!(int(1996, "E0"), "2E+003");
        assert_eq!(int(1996, "E1"), "2.0E+003");
    }

    #[test]
    fn floats_round_half_to_even() {
        assert_eq!(float(0.5, "F0"), "0");
        assert_eq!(float(0.5, "F2"), "0.50");
        assert_eq!(float(0.5, "G1"), "0.5");
        assert_eq!(float(1.5, "F0"), "2");
        assert_eq!(float(1.5, "F2"), "1.50");
        assert_eq!(float(1.5, "G1"), "2");
        assert_eq!(float(2.5, "F0"), "2");
        assert_eq!(float(2.5, "F2"), "2.50");
        assert_eq!(float(2.5, "G1"), "2");
        assert_eq!(float(3.5, "F0"), "4");
        assert_eq!(float(3.5, "F2"), "3.50");
        assert_eq!(float(3.5, "G1"), "4");
        assert_eq!(float(-2.5, "F0"), "-2");
        assert_eq!(float(-2.5, "F2"), "-2.50");
        assert_eq!(float(-2.5, "G1"), "-2");
        assert_eq!(float(-3.5, "F0"), "-4");
        assert_eq!(float(-3.5, "F2"), "-3.50");
        assert_eq!(float(-3.5, "G1"), "-4");
        assert_eq!(float(8.5, "F0"), "8");
        assert_eq!(float(8.5, "F2"), "8.50");
        assert_eq!(float(8.5, "G1"), "8");
        assert_eq!(float(9.5, "F0"), "10");
        assert_eq!(float(9.5, "F2"), "9.50");
        assert_eq!(float(9.5, "G1"), "1E+01");
        assert_eq!(float(0.125, "F0"), "0");
        assert_eq!(float(0.125, "F2"), "0.12");
        assert_eq!(float(0.125, "G1"), "0.1");
        assert_eq!(float(0.375, "F0"), "0");
        assert_eq!(float(0.375, "F2"), "0.38");
        assert_eq!(float(0.375, "G1"), "0.4");
        assert_eq!(float(0.625, "F0"), "1");
        assert_eq!(float(0.625, "F2"), "0.62");
        assert_eq!(float(0.625, "G1"), "0.6");
        assert_eq!(float(0.875, "F0"), "1");
        assert_eq!(float(0.875, "F2"), "0.88");
        assert_eq!(float(0.875, "G1"), "0.9");
        assert_eq!(float(1.5e15, "F0"), "1500000000000000");
        assert_eq!(float(1.5e15, "F2"), "1500000000000000.00");
        assert_eq!(float(1.5e15, "G1"), "2E+15");
        assert_eq!(float(2.5e15, "F0"), "2500000000000000");
        assert_eq!(float(2.5e15, "F2"), "2500000000000000.00");
        assert_eq!(float(2.5e15, "G1"), "2E+15");
    }

    #[test]
    fn number_specifier_groups_digits() {
        assert_eq!(float(1234.567, "N"), "1,234.57");
        assert_eq!(float(1234.567, "N0"), "1,235");
        assert_eq!(float(1234.567, "N1"), "1,234.6");
        assert_eq!(float(1234.567, "N3"), "1,234.567");
        assert_eq!(float(1234.0, "N"), "1,234.00");
        assert_eq!(float(1234.0, "N0"), "1,234");
        assert_eq!(float(1234.0, "N1"), "1,234.0");
        assert_eq!(float(1234.0, "N3"), "1,234.000");
        assert_eq!(float(-1234.56, "N"), "-1,234.56");
        assert_eq!(float(-1234.56, "N0"), "-1,235");
        assert_eq!(float(-1234.56, "N1"), "-1,234.6");
        assert_eq!(float(-1234.56, "N3"), "-1,234.560");
        assert_eq!(float(1234567890.12345, "N"), "1,234,567,890.12");
        assert_eq!(float(1234567890.12345, "N0"), "1,234,567,890");
        assert_eq!(float(1234567890.12345, "N1"), "1,234,567,890.1");
        assert_eq!(float(1234567890.12345, "N3"), "1,234,567,890.123");
        assert_eq!(float(0.5, "N"), "0.50");
        assert_eq!(float(0.5, "N0"), "0");
        assert_eq!(float(0.5, "N1"), "0.5");
        assert_eq!(float(0.5, "N3"), "0.500");
    }

    #[test]
    fn currency_specifier() {
        assert_eq!(float(123.456, "C"), "¤123.46");
        assert_eq!(float(123.456, "C0"), "¤123");
        assert_eq!(float(123.456, "C3"), "¤123.456");
        assert_eq!(float(123.456, "c1"), "¤123.5");
        assert_eq!(float(-123.456, "C"), "(¤123.46)");
        assert_eq!(float(-123.456, "C0"), "(¤123)");
        assert_eq!(float(-123.456, "C3"), "(¤123.456)");
        assert_eq!(float(-123.456, "c1"), "(¤123.5)");
        assert_eq!(float(0.0, "C"), "¤0.00");
        assert_eq!(float(0.0, "C0"), "¤0");
        assert_eq!(float(0.0, "C3"), "¤0.000");
        assert_eq!(float(0.0, "c1"), "¤0.0");
        assert_eq!(float(-0.4, "C"), "(¤0.40)");
        assert_eq!(float(-0.4, "C0"), "(¤0)");
        assert_eq!(float(-0.4, "C3"), "(¤0.400)");
        assert_eq!(float(-0.4, "c1"), "(¤0.4)");
        assert_eq!(float(1234567.891, "C"), "¤1,234,567.89");
        assert_eq!(float(1234567.891, "C0"), "¤1,234,568");
        assert_eq!(float(1234567.891, "C3"), "¤1,234,567.891");
        assert_eq!(float(1234567.891, "c1"), "¤1,234,567.9");
    }

    #[test]
    fn percent_specifier() {
        assert_eq!(float(1.0, "P"), "100.00 %");
        assert_eq!(float(1.0, "P0"), "100 %");
        assert_eq!(float(1.0, "P1"), "100.0 %");
        assert_eq!(float(1.0, "p2"), "100.00 %");
        assert_eq!(float(-0.39678, "P"), "-39.68 %");
        assert_eq!(float(-0.39678, "P0"), "-40 %");
        assert_eq!(float(-0.39678, "P1"), "-39.7 %");
        assert_eq!(float(-0.39678, "p2"), "-39.68 %");
        assert_eq!(float(0.0, "P"), "0.00 %");
        assert_eq!(float(0.0, "P0"), "0 %");
        assert_eq!(float(0.0, "P1"), "0.0 %");
        assert_eq!(float(0.0, "p2"), "0.00 %");
        assert_eq!(float(0.5, "P"), "50.00 %");
        assert_eq!(float(0.5, "P0"), "50 %");
        assert_eq!(float(0.5, "P1"), "50.0 %");
        assert_eq!(float(0.5, "p2"), "50.00 %");
        assert_eq!(float(1234.5678, "P"), "123,456.78 %");
        assert_eq!(float(1234.5678, "P0"), "123,457 %");
        assert_eq!(float(1234.5678, "P1"), "123,456.8 %");
        assert_eq!(float(1234.5678, "p2"), "123,456.78 %");
    }

    #[test]
    fn scientific_specifier() {
        assert_eq!(float(1052.0329112756, "E"), "1.052033E+003");
        assert_eq!(float(1052.0329112756, "E0"), "1E+003");
        assert_eq!(float(1052.0329112756, "E1"), "1.1E+003");
        assert_eq!(float(1052.0329112756, "E2"), "1.05E+003");
        assert_eq!(float(1052.0329112756, "e2"), "1.05e+003");
        assert_eq!(float(1052.0329112756, "E20"), "1.05203291127560009954E+003");
        assert_eq!(float(-1052.0329112756, "E"), "-1.052033E+003");
        assert_eq!(float(-1052.0329112756, "E0"), "-1E+003");
        assert_eq!(float(-1052.0329112756, "E1"), "-1.1E+003");
        assert_eq!(float(-1052.0329112756, "E2"), "-1.05E+003");
        assert_eq!(float(-1052.0329112756, "e2"), "-1.05e+003");
        assert_eq!(
            float(-1052.0329112756, "E20"),
            "-1.05203291127560009954E+003"
        );
        assert_eq!(float(0.0, "E"), "0.000000E+000");
        assert_eq!(float(0.0, "E0"), "0E+000");
        assert_eq!(float(0.0, "E1"), "0.0E+000");
        assert_eq!(float(0.0, "E2"), "0.00E+000");
        assert_eq!(float(0.0, "e2"), "0.00e+000");
        assert_eq!(float(0.0, "E20"), "0.00000000000000000000E+000");
        assert_eq!(float(0.1, "E"), "1.000000E-001");
        assert_eq!(float(0.1, "E0"), "1E-001");
        assert_eq!(float(0.1, "E1"), "1.0E-001");
        assert_eq!(float(0.1, "E2"), "1.00E-001");
        assert_eq!(float(0.1, "e2"), "1.00e-001");
        assert_eq!(float(0.1, "E20"), "1.00000000000000005551E-001");
        assert_eq!(float(5e-324, "E"), "4.940656E-324");
        assert_eq!(float(5e-324, "E0"), "5E-324");
        assert_eq!(float(5e-324, "E1"), "4.9E-324");
        assert_eq!(float(5e-324, "E2"), "4.94E-324");
        assert_eq!(float(5e-324, "e2"), "4.94e-324");
        assert_eq!(float(5e-324, "E20"), "4.94065645841246544177E-324");
        assert_eq!(float(f64::MAX, "E"), "1.797693E+308");
        assert_eq!(float(f64::MAX, "E0"), "2E+308");
        assert_eq!(float(f64::MAX, "E1"), "1.8E+308");
        assert_eq!(float(f64::MAX, "E2"), "1.80E+308");
        assert_eq!(float(f64::MAX, "e2"), "1.80e+308");
        assert_eq!(float(f64::MAX, "E20"), "1.79769313486231570815E+308");
    }

    #[test]
    fn decimal_specifier_is_integers_only() {
        assert_eq!(int(1234, "D"), "1234");
        assert_eq!(int(1234, "D0"), "1234");
        assert_eq!(int(1234, "D6"), "001234");
        assert_eq!(int(1234, "D25"), "0000000000000000000001234");
        assert_eq!(int(1234, "d5"), "01234");
        assert_eq!(int(-1234, "D"), "-1234");
        assert_eq!(int(-1234, "D0"), "-1234");
        assert_eq!(int(-1234, "D6"), "-001234");
        assert_eq!(int(-1234, "D25"), "-0000000000000000000001234");
        assert_eq!(int(-1234, "d5"), "-01234");
        assert_eq!(int(0, "D"), "0");
        assert_eq!(int(0, "D0"), "0");
        assert_eq!(int(0, "D6"), "000000");
        assert_eq!(int(0, "D25"), "0000000000000000000000000");
        assert_eq!(int(0, "d5"), "00000");
        assert_eq!(int(i64::MIN, "D"), "-9223372036854775808");
        assert_eq!(int(i64::MIN, "D0"), "-9223372036854775808");
        assert_eq!(int(i64::MIN, "D6"), "-9223372036854775808");
        assert_eq!(int(i64::MIN, "D25"), "-0000009223372036854775808");
        assert_eq!(int(i64::MIN, "d5"), "-9223372036854775808");
        assert_eq!(int(i64::MAX, "D"), "9223372036854775807");
        assert_eq!(int(i64::MAX, "D0"), "9223372036854775807");
        assert_eq!(int(i64::MAX, "D6"), "9223372036854775807");
        assert_eq!(int(i64::MAX, "D25"), "0000009223372036854775807");
        assert_eq!(int(i64::MAX, "d5"), "9223372036854775807");
    }

    #[test]
    fn hexadecimal_specifier() {
        assert_eq!(int(255, "X"), "FF");
        assert_eq!(int(255, "X0"), "FF");
        assert_eq!(int(255, "X4"), "00FF");
        assert_eq!(int(255, "X20"), "000000000000000000FF");
        assert_eq!(int(255, "x"), "ff");
        assert_eq!(int(255, "x8"), "000000ff");
        assert_eq!(int(-255, "X"), "FFFFFFFFFFFFFF01");
        assert_eq!(int(-255, "X0"), "FFFFFFFFFFFFFF01");
        assert_eq!(int(-255, "X4"), "FFFFFFFFFFFFFF01");
        assert_eq!(int(-255, "X20"), "0000FFFFFFFFFFFFFF01");
        assert_eq!(int(-255, "x"), "ffffffffffffff01");
        assert_eq!(int(-255, "x8"), "ffffffffffffff01");
        assert_eq!(int(0, "X"), "0");
        assert_eq!(int(0, "X0"), "0");
        assert_eq!(int(0, "X4"), "0000");
        assert_eq!(int(0, "X20"), "00000000000000000000");
        assert_eq!(int(0, "x"), "0");
        assert_eq!(int(0, "x8"), "00000000");
        assert_eq!(int(-1, "X"), "FFFFFFFFFFFFFFFF");
        assert_eq!(int(-1, "X0"), "FFFFFFFFFFFFFFFF");
        assert_eq!(int(-1, "X4"), "FFFFFFFFFFFFFFFF");
        assert_eq!(int(-1, "X20"), "0000FFFFFFFFFFFFFFFF");
        assert_eq!(int(-1, "x"), "ffffffffffffffff");
        assert_eq!(int(-1, "x8"), "ffffffffffffffff");
        assert_eq!(int(i64::MIN, "X"), "8000000000000000");
        assert_eq!(int(i64::MIN, "X0"), "8000000000000000");
        assert_eq!(int(i64::MIN, "X4"), "8000000000000000");
        assert_eq!(int(i64::MIN, "X20"), "00008000000000000000");
        assert_eq!(int(i64::MIN, "x"), "8000000000000000");
        assert_eq!(int(i64::MIN, "x8"), "8000000000000000");
        assert_eq!(int(i64::MAX, "X"), "7FFFFFFFFFFFFFFF");
        assert_eq!(int(i64::MAX, "X0"), "7FFFFFFFFFFFFFFF");
        assert_eq!(int(i64::MAX, "X4"), "7FFFFFFFFFFFFFFF");
        assert_eq!(int(i64::MAX, "X20"), "00007FFFFFFFFFFFFFFF");
        assert_eq!(int(i64::MAX, "x"), "7fffffffffffffff");
        assert_eq!(int(i64::MAX, "x8"), "7fffffffffffffff");
    }

    #[test]
    fn negative_zero_keeps_its_sign() {
        assert_eq!(float(-0.0, ""), "-0");
        assert_eq!(float(-0.0, "G"), "-0");
        assert_eq!(float(-0.0, "F2"), "-0.00");
        assert_eq!(float(-0.0, "N2"), "-0.00");
        assert_eq!(float(-0.0, "C2"), "(¤0.00)");
        assert_eq!(float(-0.0, "P2"), "-0.00 %");
        assert_eq!(float(-0.0, "E2"), "-0.00E+000");
        assert_eq!(float(-0.0, "G5"), "-0");
    }

    #[test]
    fn extreme_magnitudes() {
        assert_eq!(float(f64::MAX, "G"), "1.7976931348623157E+308");
        assert_eq!(float(f64::MAX, "G17"), "1.7976931348623157E+308");
        assert_eq!(float(f64::MAX, "E2"), "1.80E+308");
        assert_eq!(float(f64::MAX, "F2"), "179769313486231570814527423731704356798070567525844996598917476803157260780028538760589558632766878171540458953514382464234321326889464182768467546703537516986049910576551282076245490090389328944075868508455133942304583236903222948165808559332123348274797826204144723168738177180919299881250404026184124858368.00");
        assert_eq!(float(f64::MAX, "N0"), "179,769,313,486,231,570,814,527,423,731,704,356,798,070,567,525,844,996,598,917,476,803,157,260,780,028,538,760,589,558,632,766,878,171,540,458,953,514,382,464,234,321,326,889,464,182,768,467,546,703,537,516,986,049,910,576,551,282,076,245,490,090,389,328,944,075,868,508,455,133,942,304,583,236,903,222,948,165,808,559,332,123,348,274,797,826,204,144,723,168,738,177,180,919,299,881,250,404,026,184,124,858,368");
        assert_eq!(float(f64::MIN_POSITIVE, "G"), "2.2250738585072014E-308");
        assert_eq!(float(f64::MIN_POSITIVE, "G17"), "2.2250738585072014E-308");
        assert_eq!(float(f64::MIN_POSITIVE, "E2"), "2.23E-308");
        assert_eq!(float(f64::MIN_POSITIVE, "F2"), "0.00");
        assert_eq!(float(f64::MIN_POSITIVE, "N0"), "0");
        assert_eq!(float(5e-324, "G"), "5E-324");
        assert_eq!(float(5e-324, "G17"), "4.9406564584124654E-324");
        assert_eq!(float(5e-324, "E2"), "4.94E-324");
        assert_eq!(float(5e-324, "F2"), "0.00");
        assert_eq!(float(5e-324, "N0"), "0");
        assert_eq!(float(1e100, "G"), "1E+100");
        assert_eq!(float(1e100, "G17"), "1E+100");
        assert_eq!(float(1e100, "E2"), "1.00E+100");
        assert_eq!(float(1e100, "F2"), "10000000000000000159028911097599180468360808563945281389781327557747838772170381060813469985856815104.00");
        assert_eq!(float(1e100, "N0"), "10,000,000,000,000,000,159,028,911,097,599,180,468,360,808,563,945,281,389,781,327,557,747,838,772,170,381,060,813,469,985,856,815,104");
    }

    #[test]
    fn non_finite_values_use_culture_symbols() {
        assert_eq!(float(f64::NAN, ""), "NaN");
        assert_eq!(float(f64::NAN, "G"), "NaN");
        assert_eq!(float(f64::NAN, "F2"), "NaN");
        assert_eq!(float(f64::NAN, "N2"), "NaN");
        assert_eq!(float(f64::NAN, "C2"), "NaN");
        assert_eq!(float(f64::NAN, "P2"), "NaN");
        assert_eq!(float(f64::NAN, "E2"), "NaN");
        assert_eq!(float(f64::NAN, "D"), "NaN");
        assert_eq!(float(f64::NAN, "X"), "NaN");
        // The specifier is never even parsed, so one that would otherwise be
        // a custom pattern, invalid, or absurdly precise still works.
        assert_eq!(float(f64::NAN, "#,##0.00"), "NaN");
        assert_eq!(float(f64::NAN, "Q"), "NaN");
        assert_eq!(float(f64::NAN, "F1000000000"), "NaN");
        assert_eq!(float(f64::NEG_INFINITY, "#,##0.00"), "-Infinity");
        assert_eq!(float(f64::INFINITY, ""), "Infinity");
        assert_eq!(float(f64::INFINITY, "G"), "Infinity");
        assert_eq!(float(f64::INFINITY, "F2"), "Infinity");
        assert_eq!(float(f64::INFINITY, "N2"), "Infinity");
        assert_eq!(float(f64::INFINITY, "C2"), "Infinity");
        assert_eq!(float(f64::INFINITY, "P2"), "Infinity");
        assert_eq!(float(f64::INFINITY, "E2"), "Infinity");
        assert_eq!(float(f64::INFINITY, "D"), "Infinity");
        assert_eq!(float(f64::INFINITY, "X"), "Infinity");
        assert_eq!(float(f64::NEG_INFINITY, ""), "-Infinity");
        assert_eq!(float(f64::NEG_INFINITY, "G"), "-Infinity");
        assert_eq!(float(f64::NEG_INFINITY, "F2"), "-Infinity");
        assert_eq!(float(f64::NEG_INFINITY, "N2"), "-Infinity");
        assert_eq!(float(f64::NEG_INFINITY, "C2"), "-Infinity");
        assert_eq!(float(f64::NEG_INFINITY, "P2"), "-Infinity");
        assert_eq!(float(f64::NEG_INFINITY, "E2"), "-Infinity");
        assert_eq!(float(f64::NEG_INFINITY, "D"), "-Infinity");
        assert_eq!(float(f64::NEG_INFINITY, "X"), "-Infinity");
    }

    #[test]
    fn empty_spec_is_the_general_specifier() {
        assert_eq!(int(1234, ""), int(1234, "G"));
        assert_eq!(float(1234.5678, ""), float(1234.5678, "G"));
        assert_eq!(float(-0.0, ""), float(-0.0, "G"));
    }

    #[test]
    fn all_currency_patterns_render() {
        let positive = ["$1,234.50", "1,234.50$", "$ 1,234.50", "1,234.50 $"];
        let negative = [
            "($1,234.50)",
            "-$1,234.50",
            "$-1,234.50",
            "$1,234.50-",
            "(1,234.50$)",
            "-1,234.50$",
            "1,234.50-$",
            "1,234.50$-",
            "-1,234.50 $",
            "-$ 1,234.50",
            "1,234.50 $-",
            "$ 1,234.50-",
            "$ -1,234.50",
            "1,234.50- $",
            "($ 1,234.50)",
            "(1,234.50 $)",
            "$- 1,234.50",
        ];

        let mut culture = invariant().clone();
        culture.number.currency_symbol = "$";
        for (pattern, expected) in positive.iter().enumerate() {
            culture.number.currency_positive_pattern = pattern as u8;
            let got = format_number(Number::Float(1234.5), "C2", &culture).unwrap();
            assert_eq!(&got, expected, "currency positive pattern {pattern}");
        }
        for (pattern, expected) in negative.iter().enumerate() {
            culture.number.currency_negative_pattern = pattern as u8;
            let got = format_number(Number::Float(-1234.5), "C2", &culture).unwrap();
            assert_eq!(&got, expected, "currency negative pattern {pattern}");
        }
    }

    #[test]
    fn all_percent_patterns_render() {
        let positive = ["12.34 %", "12.34%", "%12.34", "% 12.34"];
        let negative = [
            "-12.34 %", "-12.34%", "-%12.34", "%-12.34", "%12.34-", "12.34-%", "12.34%-",
            "-% 12.34", "12.34 %-", "% 12.34-", "% -12.34", "12.34- %",
        ];

        let mut culture = invariant().clone();
        for (pattern, expected) in positive.iter().enumerate() {
            culture.number.percent_positive_pattern = pattern as u8;
            let got = format_number(Number::Float(0.1234), "P2", &culture).unwrap();
            assert_eq!(&got, expected, "percent positive pattern {pattern}");
        }
        for (pattern, expected) in negative.iter().enumerate() {
            culture.number.percent_negative_pattern = pattern as u8;
            let got = format_number(Number::Float(-0.1234), "P2", &culture).unwrap();
            assert_eq!(&got, expected, "percent negative pattern {pattern}");
        }
    }

    #[test]
    fn group_sizes_follow_the_culture() {
        let cases: [(&[u8], &str, &str); 5] = [
            (&[3], "123,456,789.50", "-123,456,790"),
            (&[3, 2], "12,34,56,789.50", "-12,34,56,790"),
            (&[3, 0], "123456,789.50", "-123456,790"),
            (&[4], "1,2345,6789.50", "-1,2345,6790"),
            (&[0], "123456789.50", "-123456790"),
        ];
        for (sizes, grouped, rounded) in cases {
            let mut culture = invariant().clone();
            culture.number.group_sizes = sizes;
            assert_eq!(
                format_number(Number::Float(123456789.5), "N2", &culture).unwrap(),
                grouped,
                "group sizes {sizes:?}"
            );
            assert_eq!(
                format_number(Number::Float(-123456789.5), "N0", &culture).unwrap(),
                rounded,
                "group sizes {sizes:?}"
            );
        }
    }

    #[test]
    fn custom_patterns_are_unsupported() {
        for spec in ["#,##0.00", "00", "F2x", "F-1", "yyyy", " F2", "2F"] {
            assert_eq!(
                format_number(Number::Int(1), spec, invariant()),
                Err(FormatSpecError::Unsupported(spec.to_owned())),
                "spec {spec:?}"
            );
        }
    }

    #[test]
    fn specifiers_shaped_like_a_standard_one_but_unknown_are_invalid() {
        for spec in ["Q", "q", "Z9", "W", "y3"] {
            assert_eq!(
                format_number(Number::Float(1.0), spec, invariant()),
                Err(FormatSpecError::Invalid(spec.to_owned())),
                "spec {spec:?}"
            );
        }
    }

    #[test]
    fn round_trip_specifier_on_floats_matches_the_general_one_of_the_same_case() {
        for v in [0.1, 2.675, 1e17, 5e-324, -0.0, 1.0, 1234.5678, 1e-7] {
            // `R` is `G` and `r` is `g`, so the exponent keeps the case of the
            // specifier. A float also ignores any precision on `R`/`r`.
            let upper = float(v, "G");
            let lower = float(v, "g");
            for spec in ["R", "R0", "R5", "R20"] {
                assert_eq!(float(v, spec), upper, "value {v} spec {spec:?}");
            }
            for spec in ["r", "r0", "r5", "r20"] {
                assert_eq!(float(v, spec), lower, "value {v} spec {spec:?}");
            }
        }
        assert_eq!(float(1e17, "R"), "1E+17");
        assert_eq!(float(1e17, "r"), "1e+17");
        assert_eq!(float(1e-7, "R5"), "1E-07");
        assert_eq!(float(1e-7, "r5"), "1e-07");
    }

    #[test]
    fn round_trip_specifier_on_integers_is_the_general_one_of_the_same_case() {
        // Unlike a float, an integer keeps the precision: `R<n>` is `G<n>`.
        for v in [42, -42, 0, 1234567890, i64::MIN] {
            for (r, g) in [("R", "G"), ("r", "g"), ("R5", "G5"), ("r5", "g5")] {
                assert_eq!(int(v, r), int(v, g), "value {v} spec {r:?}");
            }
        }
        assert_eq!(int(42, "R"), "42");
        assert_eq!(int(42, "r"), "42");
        assert_eq!(int(1234567890, "R"), "1234567890");
        assert_eq!(int(1234567890, "R0"), "1234567890");
        assert_eq!(int(1234567890, "R20"), "1234567890");
        assert_eq!(int(1234567890, "R5"), "1.2346E+09");
        assert_eq!(int(1234567890, "r5"), "1.2346e+09");
        assert_eq!(int(i64::MIN, "R5"), "-9.2234E+18");
        assert_eq!(
            format_number(Number::UInt(u64::MAX), "R5", invariant()).unwrap(),
            "1.8447E+19"
        );
        assert_eq!(
            format_number(Number::UInt(u64::MAX), "r", invariant()).unwrap(),
            "18446744073709551615"
        );
    }

    #[test]
    fn binary_specifier() {
        assert_eq!(int(5, "B"), "101");
        assert_eq!(int(5, "b"), "101");
        assert_eq!(int(5, "B8"), "00000101");
        assert_eq!(int(0, "B"), "0");
        assert_eq!(int(-5, "B"), "1".repeat(61) + "011");
        assert_eq!(
            format_number(Number::Float(0.1), "B", invariant()),
            Err(FormatSpecError::Invalid("B".to_owned()))
        );
    }

    #[test]
    fn unsigned_values_above_i64_max_format_exactly() {
        let max = Number::UInt(u64::MAX);
        assert_eq!(
            format_number(max, "", invariant()).unwrap(),
            "18446744073709551615"
        );
        assert_eq!(
            format_number(max, "D", invariant()).unwrap(),
            "18446744073709551615"
        );
        assert_eq!(
            format_number(max, "N0", invariant()).unwrap(),
            "18,446,744,073,709,551,615"
        );
        assert_eq!(
            format_number(max, "X", invariant()).unwrap(),
            "FFFFFFFFFFFFFFFF"
        );
    }

    #[test]
    fn integer_only_specifiers_reject_floats() {
        for spec in ["D", "D5", "d", "X", "X8", "x"] {
            assert_eq!(
                format_number(Number::Float(1.0), spec, invariant()),
                Err(FormatSpecError::Invalid(spec.to_owned())),
                "spec {spec:?}"
            );
        }
    }

    #[test]
    fn precision_beyond_nine_digits_is_invalid() {
        assert_eq!(
            format_number(Number::Int(1), "F1000000000", invariant()),
            Err(FormatSpecError::Invalid("F1000000000".to_owned()))
        );
        assert_eq!(
            format_number(Number::Int(1), "F000000002", invariant()).unwrap(),
            "1.00"
        );
    }

    #[test]
    fn precision_digits_may_be_zero_padded() {
        assert_eq!(int(1, "F002"), "1.00");
        assert_eq!(int(1, "D005"), "00001");
    }

    // Every expectation below is the output of .NET 10
    // `value.ToString(spec, CultureInfo.GetCultureInfo(name))`, transcribed
    // from a probe that escaped anything outside printable ASCII — which is
    // the only way to see that `fr-FR` groups with U+202F and `pt-PT` with
    // U+00A0, or that `sv` negates with U+2212 rather than a hyphen.
    mod cultures {
        use super::*;
        use crate::fmt::culture;

        fn num(name: &str, v: f64, spec: &str) -> String {
            let culture = culture::get(name).expect("a culture the port ships");
            format_number(Number::Float(v), spec, culture).expect("standard spec")
        }

        fn int_num(name: &str, v: i64, spec: &str) -> String {
            let culture = culture::get(name).expect("a culture the port ships");
            format_number(Number::Int(v), spec, culture).expect("standard spec")
        }

        fn nan(name: &str) -> String {
            num(name, f64::NAN, "")
        }

        fn infinities(name: &str) -> (String, String) {
            (
                num(name, f64::INFINITY, ""),
                num(name, f64::NEG_INFINITY, ""),
            )
        }

        /// Decimal comma, dot groups, currency after the number, percent with
        /// a space — and `N`/`P` defaulting to *three* decimals, which is ICU
        /// data rather than the invariant culture's two.
        #[test]
        fn de_de() {
            assert_eq!(num("de-DE", -1234567.891, "N"), "-1.234.567,891");
            assert_eq!(num("de-DE", -1234567.891, "N0"), "-1.234.568");
            assert_eq!(num("de-DE", -1234567.891, "N3"), "-1.234.567,891");
            assert_eq!(num("de-DE", -1234567.891, "C"), "-1.234.567,89 \u{20ac}");
            assert_eq!(num("de-DE", -1234567.891, "C0"), "-1.234.568 \u{20ac}");
            assert_eq!(num("de-DE", -1234567.891, "C3"), "-1.234.567,891 \u{20ac}");
            assert_eq!(num("de-DE", -1234567.891, "P"), "-123.456.789,100 %");
            assert_eq!(num("de-DE", -1234567.891, "P1"), "-123.456.789,1 %");
            assert_eq!(num("de-DE", -1234567.891, "F2"), "-1234567,89");
            assert_eq!(num("de-DE", -1234567.891, "E2"), "-1,23E+006");
            assert_eq!(num("de-DE", -1234567.891, "G"), "-1234567,891");
            assert_eq!(num("de-DE", 1234567.891, "N"), "1.234.567,891");
            assert_eq!(num("de-DE", 1234567.891, "C"), "1.234.567,89 \u{20ac}");
            assert_eq!(num("de-DE", 1234567.891, "P"), "123.456.789,100 %");
            assert_eq!(int_num("de-DE", -42, "N0"), "-42");
            assert_eq!(nan("de-DE"), "NaN");
            assert_eq!(
                infinities("de-DE"),
                ("\u{221e}".to_owned(), "-\u{221e}".to_owned())
            );
        }

        /// U+202F NARROW NO-BREAK SPACE between groups — not U+00A0, and not a
        /// plain space.
        #[test]
        fn fr_fr() {
            assert_eq!(
                num("fr-FR", -1234567.891, "N"),
                "-1\u{202f}234\u{202f}567,891"
            );
            assert_eq!(num("fr-FR", -1234567.891, "N0"), "-1\u{202f}234\u{202f}568");
            assert_eq!(
                num("fr-FR", -1234567.891, "C"),
                "-1\u{202f}234\u{202f}567,89 \u{20ac}"
            );
            assert_eq!(
                num("fr-FR", -1234567.891, "C3"),
                "-1\u{202f}234\u{202f}567,891 \u{20ac}"
            );
            assert_eq!(
                num("fr-FR", -1234567.891, "P"),
                "-123\u{202f}456\u{202f}789,100 %"
            );
            assert_eq!(num("fr-FR", -1234567.891, "F2"), "-1234567,89");
            assert_eq!(num("fr-FR", -1234567.891, "E2"), "-1,23E+006");
            assert_eq!(
                num("fr-FR", 1234567.891, "N"),
                "1\u{202f}234\u{202f}567,891"
            );
            assert_eq!(
                num("fr-FR", 1234567.891, "C"),
                "1\u{202f}234\u{202f}567,89 \u{20ac}"
            );
        }

        /// The other no-break space: `pt-PT` and `ru` group with U+00A0 while
        /// `fr-FR` uses U+202F, so the two cannot share one "space" symbol.
        #[test]
        fn pt_pt_and_ru_group_with_a_plain_no_break_space() {
            assert_eq!(num("pt-PT", -1234567.891, "N"), "-1\u{a0}234\u{a0}567,891");
            assert_eq!(
                num("pt-PT", -1234567.891, "P"),
                "-123\u{a0}456\u{a0}789,100%"
            );
            assert_eq!(num("ru", -1234567.891, "N"), "-1\u{a0}234\u{a0}567,891");
            assert_eq!(num("ru", -1234567.891, "P"), "-123\u{a0}456\u{a0}789,100 %");
            assert_eq!(
                nan("ru"),
                "\u{43d}\u{435}\u{a0}\u{447}\u{438}\u{441}\u{43b}\u{43e}"
            );
        }

        /// Zero currency decimals (króna has no subunit) and a percent sign
        /// with no space — where `de-DE` puts one.
        #[test]
        fn is_is() {
            assert_eq!(num("is-IS", -1234567.891, "N"), "-1.234.567,891");
            assert_eq!(num("is-IS", -1234567.891, "N0"), "-1.234.568");
            assert_eq!(num("is-IS", -1234567.891, "C"), "-1.234.568 kr.");
            assert_eq!(num("is-IS", -1234567.891, "C0"), "-1.234.568 kr.");
            // An explicit precision still wins over CurrencyDecimalDigits.
            assert_eq!(num("is-IS", -1234567.891, "C3"), "-1.234.567,891 kr.");
            assert_eq!(num("is-IS", -1234567.891, "P"), "-123.456.789,100%");
            assert_eq!(num("is-IS", -1234567.891, "P1"), "-123.456.789,1%");
            assert_eq!(num("is-IS", -1234567.891, "E2"), "-1,23E+006");
            assert_eq!(num("is-IS", 1234567.891, "C"), "1.234.568 kr.");
            // `is` (the neutral culture) keeps the placeholder currency sign.
            assert_eq!(num("is", 1234567.891, "C"), "1.234.567,89 \u{a4}");
        }

        /// Arabic-Indic separators, a currency symbol ending in an RTL mark,
        /// and a negative sign and exponent sign that carry U+061C ARABIC
        /// LETTER MARK. Digits stay ASCII: .NET Core never substitutes native
        /// digits when it formats a number.
        #[test]
        fn ar_sa() {
            assert_eq!(
                num("ar-SA", -1234567.891, "N"),
                "\u{61c}-1\u{66c}234\u{66c}567\u{66b}891"
            );
            assert_eq!(
                num("ar-SA", -1234567.891, "N0"),
                "\u{61c}-1\u{66c}234\u{66c}568"
            );
            assert_eq!(
                num("ar-SA", -1234567.891, "C"),
                "\u{61c}-1\u{66c}234\u{66c}567\u{66b}89 \u{631}.\u{633}.\u{200f}"
            );
            assert_eq!(
                num("ar-SA", -1234567.891, "P"),
                "\u{61c}-123\u{66c}456\u{66c}789\u{66b}100\u{66a}\u{61c}"
            );
            assert_eq!(
                num("ar-SA", -1234567.891, "E2"),
                "\u{61c}-1\u{66b}23E\u{61c}+006"
            );
            assert_eq!(num("ar-SA", 0.1234, "E2"), "1\u{66b}23E\u{61c}-001");
            assert_eq!(
                num("ar-SA", 1234567.891, "N"),
                "1\u{66c}234\u{66c}567\u{66b}891"
            );
            assert_eq!(int_num("ar-SA", -42, "N0"), "\u{61c}-42");
            assert_eq!(
                nan("ar-SA"),
                "\u{644}\u{64a}\u{633}\u{a0}\u{631}\u{642}\u{645}\u{64b}\u{627}"
            );
            assert_eq!(
                infinities("ar-SA"),
                ("\u{221e}".to_owned(), "\u{61c}-\u{221e}".to_owned())
            );
        }

        /// `sv`, `fi` and `nb` negate with U+2212 MINUS SIGN, so a caller that
        /// splits on `'-'` sees nothing.
        #[test]
        fn nordic_cultures_use_a_real_minus_sign() {
            assert_eq!(
                num("sv", -1234567.891, "N"),
                "\u{2212}1\u{a0}234\u{a0}567,891"
            );
            assert_eq!(num("sv", -1234567.891, "F2"), "\u{2212}1234567,89");
            assert_eq!(num("sv", -1234567.891, "E2"), "\u{2212}1,23E+006");
            assert_eq!(int_num("sv", -42, "N0"), "\u{2212}42");
            assert_eq!(
                infinities("sv"),
                ("\u{221e}".to_owned(), "\u{2212}\u{221e}".to_owned())
            );
            assert_eq!(int_num("fi", -42, "N0"), "\u{2212}42");
            assert_eq!(int_num("nb", -42, "N0"), "\u{2212}42");
        }

        /// The pattern indices the .NET enumerations encode, each picked
        /// because a different culture lands on a different arm of them.
        #[test]
        fn currency_and_percent_patterns() {
            // CurrencyPositivePattern 0 / CurrencyNegativePattern 1: `-$n`.
            assert_eq!(num("en-US", -1234567.891, "C"), "-$1,234,567.89");
            assert_eq!(num("en-US", 1234567.891, "C"), "$1,234,567.89");
            // 2 / 2: `$ n` positive but `$-n` negative — the space vanishes.
            assert_eq!(num("de-CH", -1234567.891, "C"), "CHF-1'234'567.89");
            assert_eq!(num("de-CH", 1234567.891, "C"), "CHF 1'234'567.89");
            // 2 / 12: `$ n` and `$ -n`.
            assert_eq!(num("nl", -1234567.891, "C"), "\u{a4} -1.234.567,89");
            assert_eq!(num("nl", 1234567.891, "C"), "\u{a4} 1.234.567,89");
            // 2 / 9: `$ n` and `-$ n`.
            assert_eq!(num("de-AT", -1234567.891, "C"), "-\u{20ac} 1.234.567,89");
            assert_eq!(num("de-AT", 1234567.891, "C"), "\u{20ac} 1.234.567,89");
            // PercentPositivePattern 2: the sign leads the number.
            assert_eq!(num("tr", -1234567.891, "P"), "-%123.456.789,100");
            assert_eq!(num("tr", 1234567.891, "P"), "%123.456.789,100");
            // de-CH groups with an apostrophe.
            assert_eq!(num("de-CH", -1234567.891, "N"), "-1'234'567.891");
        }
    }
}
