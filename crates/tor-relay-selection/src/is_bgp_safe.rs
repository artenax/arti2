//! Logic for checking BGP safety using
//! ASN data from CSV and IP-to-ASN mapping.

use ip2asn::{Builder, IpAsnMap};
use std::net::IpAddr;

/// A trait for checking if an IP address is considered BGP-safe.
pub trait IsBgpSafeChecker: std::fmt::Debug + Send + Sync {
    /// Returns true if the provided IP address is associated
    /// with a BGP-safe ASN according to the given data sources.
    fn is_bgp_safe(&self, ip: &IpAddr) -> bool;
}

/// Error type for dealing with reading and parsing data sources.
#[derive(thiserror::Error, Debug)]
pub enum IsBgpSafeCheckerError {
    /// An error occured while loading the CSV file containing safe operators.
    #[error("failed to load CSV containing safe operators: {0}")]
    SafeOperatorsCsv(#[from] csv::Error),
    /// An error occured while loading the TSV file containing IP-to-ASN data.
    #[error("failed to load TSV containing data for ip2asn conversion: {0}")]
    Ip2AsnTsv(#[from] ip2asn::Error),
    /// A required column was missing from the CSV file.
    #[error("missing expected column in CSV: {0}")]
    MissingColumn(String),
    /// An I/O error occured while reading one of the data files.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// An implementation of IsBgpSafeChecker that uses a CSV and TSV files
/// as data source.
#[derive(Debug)]
pub struct IsBgpSafeCheckerCsv {
    safe_asns: std::collections::HashSet<u32>,
    ip2asn_map: IpAsnMap,
}

impl IsBgpSafeCheckerCsv {
    /// Create a new IsBgpSafeCheckerCsv from the given CSV and TSV files.
    /// The object will contain all the safe ASNs in `safe_asns` and also a mapper
    /// to retrieve the ASN for a given IP address.
    pub fn from_files(
        asns_csv_path: &std::path::Path,
        ip2asn_tsv_path: &std::path::Path,
    ) -> Result<Self, IsBgpSafeCheckerError> {
        // Const for reading the CSV file.
        const STATUS_HEADER: &str = "status";
        const ASN_HEADER: &str = "asn";
        const SAFE_STATUS: &str = "safe";

        // Initialize CSV reader and find indices of relevant columns.
        let mut csv_reader = csv::Reader::from_path(asns_csv_path)?;
        let headers = csv_reader.headers()?.clone();
        let status_idx = headers
            .iter()
            .position(|h| h == STATUS_HEADER)
            .ok_or_else(|| IsBgpSafeCheckerError::MissingColumn(STATUS_HEADER.into()))?;
        let asn_idx = headers
            .iter()
            .position(|h| h == ASN_HEADER)
            .ok_or_else(|| IsBgpSafeCheckerError::MissingColumn(ASN_HEADER.into()))?;

        // Read safe ASNs from the CSV file.
        let mut safe_asns = std::collections::HashSet::new();
        for row in csv_reader.records() {
            let row = row?;
            let (status, asn) = (row.get(status_idx), row.get(asn_idx));
            if status == Some(SAFE_STATUS) {
                if let Some(safe_asn) = asn {
                    if let Ok(asn_identifier) = safe_asn.parse::<u32>() {
                        safe_asns.insert(asn_identifier);
                    }
                }
            }
        }

        // Create the IP-to-ASN map from the TSV file
        let ip2asn_file = std::fs::File::open(ip2asn_tsv_path)?;
        let ip2asn_map = Builder::new()
            .with_source(std::io::BufReader::new(ip2asn_file))?
            .build()?;

        Ok(Self {
            safe_asns,
            ip2asn_map,
        })
    }
}

impl IsBgpSafeChecker for IsBgpSafeCheckerCsv {
    fn is_bgp_safe(&self, ip: &IpAddr) -> bool {
        self.ip2asn_map
            .lookup(*ip)
            .map(|info| self.safe_asns.contains(&info.asn))
            .unwrap_or(false)
    }
}
