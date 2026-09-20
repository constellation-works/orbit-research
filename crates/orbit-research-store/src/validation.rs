//! Owner schema and whole-corpus graph validation.
use crate::{Error, Result};
use orbit_research_common::Record;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct Contract {
    pub(crate) schema: Value,
    validator: jsonschema::JSONSchema,
}
impl Contract {
    pub(crate) fn compile(schema: Value) -> Result<Self> {
        if !schema["x-observatory"]["kinds"].is_object() {
            return Err(Error::Invalid(
                "Corpus does not export the Observatory record contract".into(),
            ));
        }
        let validator = jsonschema::JSONSchema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .compile(&schema)
            .map_err(|e| Error::Invalid(format!("Invalid owner schema: {e}")))?;
        Ok(Self { schema, validator })
    }
    pub(crate) fn validate(&self, metadata: &Value, path: &str) -> Result<()> {
        if let Err(errors) = self.validator.validate(metadata) {
            return Err(Error::Invalid(format!(
                "{path}: {}",
                errors.map(|e| e.to_string()).collect::<Vec<_>>().join("; ")
            )));
        }
        Ok(())
    }
}

pub(crate) fn validate_records(records: &BTreeMap<String, Record>) -> Result<()> {
    check_numbering(records)?;
    for record in records.values() {
        for (field, allowed) in [
            ("derived_from", &["Q", "H", "T", "R"] as &[&str]),
            ("answered_by", &["H", "R"]),
            ("tests", &["H"]),
            ("claims", &["H"]),
            ("supersedes", &["T"]),
        ] {
            if let Some(refs) = record.metadata[field].as_array() {
                for target in refs {
                    let target = target
                        .as_str()
                        .ok_or_else(|| Error::Invalid("Invalid reference".into()))?;
                    let Some(target_record) = records.get(target) else {
                        return Err(Error::Invalid(format!(
                            "{} {field} references missing {target}",
                            record.id
                        )));
                    };
                    if !allowed.contains(&target_record.kind.as_str()) {
                        return Err(Error::Invalid(format!(
                            "{} {field} references {} record {target}; expected {}",
                            record.id,
                            target_record.kind,
                            allowed.join(" or ")
                        )));
                    }
                }
            }
        }
        if let Some(assessments) = record.metadata["assessments"].as_array() {
            for (index, assessment) in assessments.iter().enumerate() {
                let target = assessment["research"].as_str().ok_or_else(|| {
                    Error::Invalid(format!(
                        "{} assessments[{index}] has invalid research reference",
                        record.id
                    ))
                })?;
                let Some(target_record) = records.get(target) else {
                    return Err(Error::Invalid(format!(
                        "{} assessments[{index}].research references missing {target}",
                        record.id
                    )));
                };
                if target_record.kind != "R" {
                    return Err(Error::Invalid(format!(
                        "{} assessments[{index}].research references {} record {target}; expected R",
                        record.id, target_record.kind
                    )));
                }
                if let (Some(current), Some(revision)) = (
                    record.metadata["revision"].as_u64(),
                    assessment["revision"].as_u64(),
                ) && revision > current
                {
                    return Err(Error::Invalid(format!(
                        "{} assessments[{index}] is against revision {revision}, beyond hypothesis revision {current}",
                        record.id
                    )));
                }
            }
        }
    }
    let mut done = BTreeSet::new();
    for id in records.keys() {
        check_lineage(id, records, &mut BTreeSet::new(), &mut done)?;
    }
    Ok(())
}

fn check_numbering(records: &BTreeMap<String, Record>) -> Result<()> {
    let mut numbers: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for record in records.values() {
        let number = record.id[1..]
            .parse::<u32>()
            .map_err(|_| Error::Invalid(format!("Invalid record ID: {}", record.id)))?;
        numbers.entry(record.kind.clone()).or_default().push(number);
    }
    for (kind, mut values) in numbers {
        values.sort_unstable();
        for (index, number) in values.into_iter().enumerate() {
            let expected = index as u32 + 1;
            if number != expected {
                return Err(Error::Invalid(format!(
                    "Non-monotonic IDs: expected {kind}{expected:03}, found {kind}{number:03}"
                )));
            }
        }
    }
    Ok(())
}

fn check_lineage(
    id: &str,
    records: &BTreeMap<String, Record>,
    visiting: &mut BTreeSet<String>,
    done: &mut BTreeSet<String>,
) -> Result<()> {
    if done.contains(id) {
        return Ok(());
    }
    if !visiting.insert(id.into()) {
        return Err(Error::Invalid(format!("Lineage cycle at {id}")));
    }
    let record = records
        .get(id)
        .ok_or_else(|| Error::Invalid(format!("Missing lineage record {id}")))?;
    if let Some(parents) = record.metadata["derived_from"].as_array() {
        for parent in parents {
            let parent = parent
                .as_str()
                .ok_or_else(|| Error::Invalid("Invalid lineage ID".into()))?;
            check_lineage(parent, records, visiting, done)?;
        }
    }
    visiting.remove(id);
    done.insert(id.into());
    Ok(())
}
