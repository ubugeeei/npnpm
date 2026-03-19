#[derive(Debug, PartialEq, Eq)]
pub struct ParsedPackageSpec<'a> {
    pub name: &'a str,
    pub specifier: Option<&'a str>,
}

impl<'a> ParsedPackageSpec<'a> {
    pub fn parse(input: &'a str) -> Self {
        let separator = input
            .char_indices()
            .skip(1)
            .filter_map(|(index, ch)| (ch == '@').then_some(index))
            .last()
            .filter(|index| index + 1 < input.len());

        let (name, specifier) = separator
            .map(|index| (&input[..index], Some(&input[index + 1..])))
            .unwrap_or((input, None));

        Self { name, specifier }
    }
}

#[cfg(test)]
mod tests {
    use super::ParsedPackageSpec;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_parse_unscoped_package_specs() {
        assert_eq!(
            ParsedPackageSpec::parse("react"),
            ParsedPackageSpec { name: "react", specifier: None },
        );
        assert_eq!(
            ParsedPackageSpec::parse("react@18.3.1"),
            ParsedPackageSpec { name: "react", specifier: Some("18.3.1") },
        );
        assert_eq!(
            ParsedPackageSpec::parse("react@latest"),
            ParsedPackageSpec { name: "react", specifier: Some("latest") },
        );
    }

    #[test]
    fn should_parse_scoped_package_specs() {
        assert_eq!(
            ParsedPackageSpec::parse("@scope/example"),
            ParsedPackageSpec { name: "@scope/example", specifier: None },
        );
        assert_eq!(
            ParsedPackageSpec::parse("@scope/example@1.2.3"),
            ParsedPackageSpec { name: "@scope/example", specifier: Some("1.2.3") },
        );
        assert_eq!(
            ParsedPackageSpec::parse("@scope/example@^1"),
            ParsedPackageSpec { name: "@scope/example", specifier: Some("^1") },
        );
    }
}
