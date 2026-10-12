//! Arguments several subcommands share, and their parsers.

use supvan_proto::bitmap::CardPattern;
use supvan_proto::buffer::Density;
use supvan_proto::rfid::{DEFAULT_LENGTH_MM, DEFAULT_WIDTH_MM};
use supvan_proto::status::DEFAULT_LABEL_GAP_MM;

/// Which printer to talk to.
#[derive(clap::Args)]
pub struct Target {
    /// Bluetooth address, `ble://<address>`, or /dev/hidrawN path
    pub target: String,
}

/// Label size, for subcommands that compose an image.
#[derive(clap::Args, Clone, Copy)]
pub struct LabelSize {
    /// Label width across the printhead, mm
    #[arg(long, default_value_t = DEFAULT_WIDTH_MM)]
    pub width: u8,
    /// Label length along the feed direction, mm
    #[arg(long, default_value_t = DEFAULT_LENGTH_MM)]
    pub length: u8,
}

/// Label geometry, for subcommands that write a material record.
#[derive(clap::Args, Clone, Copy)]
pub struct LabelArgs {
    #[command(flatten)]
    pub size: LabelSize,
    /// Inter-label gap, mm
    #[arg(long, default_value_t = DEFAULT_LABEL_GAP_MM)]
    pub gap: u8,
}

/// A density: `N` sets both trims, `BLACK:RED` sets them apart.
pub fn parse_density(s: &str) -> Result<Density, String> {
    match s.split_once(':') {
        Some((black, red)) => Ok(Density {
            black: black.parse().map_err(|_| format!("bad black `{black}`"))?,
            red: red.parse().map_err(|_| format!("bad red `{red}`"))?,
        }),
        None => Ok(Density::uniform(
            s.parse().map_err(|_| format!("bad density `{s}`"))?,
        )),
    }
}

/// A `heat5:heat40` pair, e.g. `1700:1200`.
pub fn parse_heat_pair(s: &str) -> Result<(u16, u16), String> {
    let (h5, h40) = s
        .split_once(':')
        .ok_or_else(|| format!("expected `heat5:heat40`, got `{s}`"))?;
    Ok((
        h5.parse().map_err(|_| format!("bad heat5 `{h5}`"))?,
        h40.parse().map_err(|_| format!("bad heat40 `{h40}`"))?,
    ))
}

/// The two-colour test-card pattern name.
pub fn parse_pattern(s: &str) -> Result<CardPattern, String> {
    match s {
        "bars" => Ok(CardPattern::Bars),
        "stripes" => Ok(CardPattern::Stripes),
        other => Err(format!("unknown pattern `{other}` (bars|stripes)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `N` sets both trims; `BLACK:RED` drives them apart. The split form is the
    /// whole point of the type — black and red live in different header fields.
    #[test]
    fn density_accepts_both_forms() {
        assert_eq!(parse_density("6"), Ok(Density::uniform(6)));
        assert_eq!(parse_density("3:12"), Ok(Density { black: 3, red: 12 }));
        assert!(parse_density("3:").is_err());
        assert!(parse_density("x").is_err());
    }

    #[test]
    fn heat_pair_needs_a_colon() {
        assert!(parse_heat_pair("1700").is_err());
        assert!(parse_heat_pair("1700:abc").is_err());
        assert_eq!(parse_heat_pair("1700:1200"), Ok((1700, 1200)));
    }
}
