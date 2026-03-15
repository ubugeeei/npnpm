use crate::{ParsePkgNameSuffixError, ParsePkgVerPeerError, PkgName, PkgNameSuffix, PkgVerPeer};
use std::fmt::Write;

/// Syntax: `{name}@{version}({peers})`
///
/// Example: `react-json-view@1.21.3(@types/react@17.0.49)(react-dom@17.0.2)(react@17.0.2)`
///
/// **NOTE:** The suffix isn't guaranteed to be correct. It is only assumed to be.
pub type PkgNameVerPeer = PkgNameSuffix<PkgVerPeer>;

/// Error when parsing [`PkgNameVerPeer`] from a string.
pub type ParsePkgNameVerPeerError = ParsePkgNameSuffixError<ParsePkgVerPeerError>;

impl PkgNameVerPeer {
    /// Construct the name of the corresponding subdirectory in the virtual store directory.
    pub fn to_virtual_store_name(&self) -> String {
        Self::to_virtual_store_name_from_parts(&self.name, &self.suffix)
    }

    /// Construct the virtual store name from borrowed package parts.
    pub fn to_virtual_store_name_from_parts(name: &PkgName, ver_peer: &PkgVerPeer) -> String {
        let mut result =
            String::with_capacity(estimated_virtual_store_name_capacity(name, ver_peer));

        push_virtual_store_name(&mut result, name);
        result.push('@');
        write!(&mut result, "{}", ver_peer.version()).expect("write version to string");
        push_virtual_store_peer_suffix(&mut result, ver_peer.peer());

        result
    }
}

fn estimated_virtual_store_name_capacity(name: &PkgName, ver_peer: &PkgVerPeer) -> usize {
    name.scope.as_deref().map_or(0, |scope| scope.len() + 2)
        + name.bare.len()
        + ver_peer.peer().len()
        + 17
}

fn push_virtual_store_name(buf: &mut String, name: &PkgName) {
    if let Some(scope) = name.scope.as_deref() {
        buf.push('@');
        buf.push_str(scope);
        buf.push('+');
    }

    buf.push_str(&name.bare);
}

fn push_virtual_store_peer_suffix(buf: &mut String, peer: &str) {
    for ch in peer.chars() {
        match ch {
            '/' => buf.push('+'),
            '(' => buf.push('_'),
            ')' => {}
            _ => buf.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn name_peer_ver(name: &str, peer_ver: &str) -> PkgNameVerPeer {
        let peer_ver = peer_ver.to_string().parse().unwrap();
        PkgNameVerPeer::new(name.parse().unwrap(), peer_ver)
    }

    #[test]
    fn parse() {
        fn case(input: &'static str, expected: PkgNameVerPeer) {
            eprintln!("CASE: {input:?}");
            let received: PkgNameVerPeer = input.parse().unwrap();
            assert_eq!(&received, &expected);
        }

        case(
            "react-json-view@1.21.3(@types/react@17.0.49)(react-dom@17.0.2)(react@17.0.2)",
            name_peer_ver(
                "react-json-view",
                "1.21.3(@types/react@17.0.49)(react-dom@17.0.2)(react@17.0.2)",
            ),
        );
        case("react-json-view@1.21.3", name_peer_ver("react-json-view", "1.21.3"));
        case(
            "@algolia/autocomplete-core@1.9.3(@algolia/client-search@4.18.0)(algoliasearch@4.18.0)(search-insights@2.6.0)",
            name_peer_ver(
                "@algolia/autocomplete-core",
                "1.9.3(@algolia/client-search@4.18.0)(algoliasearch@4.18.0)(search-insights@2.6.0)",
            ),
        );
        case(
            "@algolia/autocomplete-core@1.9.3",
            name_peer_ver("@algolia/autocomplete-core", "1.9.3"),
        );
    }

    #[test]
    fn to_virtual_store_name() {
        fn case(input: &'static str, expected: &'static str) {
            eprintln!("CASE: {input:?}");
            let name_ver_peer: PkgNameVerPeer = input.parse().unwrap();
            dbg!(&name_ver_peer);
            let received = name_ver_peer.to_virtual_store_name();
            assert_eq!(received, expected);
        }

        case("ts-node@10.9.1", "ts-node@10.9.1");
        case(
            "ts-node@10.9.1(@types/node@18.7.19)(typescript@5.1.6)",
            "ts-node@10.9.1_@types+node@18.7.19_typescript@5.1.6",
        );
        case(
            "@babel/plugin-proposal-object-rest-spread@7.12.1",
            "@babel+plugin-proposal-object-rest-spread@7.12.1",
        );
        case(
            "@babel/plugin-proposal-object-rest-spread@7.12.1(@babel/core@7.12.9)",
            "@babel+plugin-proposal-object-rest-spread@7.12.1_@babel+core@7.12.9",
        );
    }
}
