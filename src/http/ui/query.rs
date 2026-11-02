//! Native form translation is confined to marked UI requests; API grammar is untouched.
use crate::{
    error::Error,
    service::matrix::{MatrixQuery, Selector},
};

pub fn fields(raw: &str) -> Vec<(String, String)> {
    let mut url = reqwest::Url::parse("http://ui.invalid/").expect("constant URL");
    url.set_query(Some(raw));
    url.query_pairs().into_owned().collect()
}

pub fn field<'a>(fields: &'a [(String, String)], name: &str) -> &'a str {
    fields
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
        .unwrap_or("")
}

pub fn encode_fields(fields: &[(String, String)]) -> String {
    let mut url = reqwest::Url::parse("http://ui.invalid/").expect("constant URL");
    url.query_pairs_mut()
        .extend_pairs(fields.iter().map(|(k, v)| (k, v)));
    url.query()
        .expect("query_pairs_mut creates a query")
        .to_owned()
}

pub fn selector_fields(
    name: &str,
    mode: &str,
    value: &str,
) -> Result<Vec<(String, String)>, Error> {
    let mut fields = vec![("q[][pacticipant]".into(), name.into())];
    let extra = match mode {
        "all" => vec![],
        "latest" => vec![("q[][latest]", "true")],
        "main" => vec![("q[][latest]", "true"), ("q[][mainBranch]", "true")],
        "version" | "branch" | "tag" => {
            if value.is_empty() {
                return Err(Error::BadRequest(
                    "The selected filter requires a value.".into(),
                ));
            }
            match mode {
                "version" => vec![("q[][version]", value)],
                "branch" => vec![("q[][latest]", "true"), ("q[][branch]", value)],
                _ => vec![("q[][latest]", "true"), ("q[][tag]", value)],
            }
        }
        _ => return Err(Error::BadRequest("Unknown selector mode.".into())),
    };
    fields.extend(extra.into_iter().map(|(k, v)| (k.into(), v.into())));
    Ok(fields)
}

pub fn matrix_query(
    raw: &str,
    consumer: &str,
    provider: &str,
) -> Result<(MatrixQuery, String), Error> {
    let fields = fields(raw);
    let canonical = if field(&fields, "ui_form") == "matrix" {
        let mut out = selector_fields(
            consumer,
            field(&fields, "consumer_mode"),
            field(&fields, "consumer_value"),
        )?;
        out.extend(selector_fields(
            provider,
            field(&fields, "provider_mode"),
            field(&fields, "provider_value"),
        )?);
        match field(&fields, "latestby") {
            "none" => {}
            value @ ("cvpv" | "cvp" | "cp") => out.push(("latestby".into(), value.into())),
            _ => return Err(Error::BadRequest("Unknown latestby grouping.".into())),
        }
        encode_fields(&out)
    } else {
        raw.to_owned()
    };
    let mut q = MatrixQuery::parse(Some(&canonical));
    if q.selectors
        .iter()
        .any(|s| s.pacticipant != consumer && s.pacticipant != provider)
    {
        return Err(Error::BadRequest(
            "Selectors must belong to this consumer/provider pair.".into(),
        ));
    }
    let mut missing = vec![consumer, provider];
    for selector in &q.selectors {
        match missing
            .iter()
            .position(|name| *name == selector.pacticipant)
        {
            Some(index) => {
                missing.remove(index);
            }
            None => {
                return Err(Error::BadRequest(
                    "Specify each member of the pair only once.".into(),
                ));
            }
        }
    }
    for name in missing {
        q.selectors.push(Selector {
            pacticipant: name.into(),
            ..Default::default()
        });
    }
    Ok((q, canonical))
}

pub fn explanation_query(raw: &str) -> Result<(MatrixQuery, String), Error> {
    let fields = fields(raw);
    let canonical = if field(&fields, "ui_form") == "explanation" {
        let name = field(&fields, "pacticipant");
        let target = field(&fields, "target_value");
        if name.is_empty() || target.is_empty() {
            return Err(Error::BadRequest(
                "Pacticipant and target are required.".into(),
            ));
        }
        let mut out = selector_fields(
            name,
            field(&fields, "selector_mode"),
            field(&fields, "selector_value"),
        )?;
        let key = match field(&fields, "target_mode") {
            "environment" => "environment",
            "tag" => "to",
            _ => return Err(Error::BadRequest("Unknown target mode.".into())),
        };
        out.push((key.into(), target.into()));
        for (_, ignore) in fields.iter().filter(|(key, _)| key == "ignore") {
            if ignore.is_empty() {
                continue;
            }
            let (name, version) = ignore
                .split_once('=')
                .map(|(n, v)| (n, Some(v)))
                .unwrap_or((ignore, None));
            if name.is_empty() {
                return Err(Error::BadRequest(
                    "An ignore must name a pacticipant.".into(),
                ));
            }
            out.push(("ignore[][pacticipant]".into(), name.into()));
            if let Some(version) = version {
                out.push(("ignore[][version]".into(), version.into()));
            }
        }
        encode_fields(&out)
    } else {
        raw.to_owned()
    };
    Ok((MatrixQuery::parse(Some(&canonical)), canonical))
}
