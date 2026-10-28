//! Bounded Array mutation policy. Values and object identity remain in host storage.
//! Original implementation of Adobe's Array.insertAt/removeAt index contracts.
//! https://help.adobe.com/en_US/FlashPlatform/reference/actionscript/3/Array.html

use serde::Serialize;

pub const MAX_ARRAY_LENGTH: u32 = 1_000_000;

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct MutationPlan {
    pub index: u32,
    pub remove: u32,
    pub insert: bool,
}

pub fn mutation_plan(method: &str, length: u32, index: i32) -> Result<MutationPlan, String> {
    if length > MAX_ARRAY_LENGTH {
        return Err("Array mutation exceeds the 1000000-element profile".into());
    }
    let position = if index < 0 {
        i64::from(length) + i64::from(index)
    } else {
        i64::from(index)
    };
    let index = position.clamp(0, i64::from(length)) as u32;
    match method {
        "insertAt" if length == MAX_ARRAY_LENGTH => {
            Err("Array insertion exceeds the 1000000-element profile".into())
        }
        "insertAt" => Ok(MutationPlan {
            index,
            remove: 0,
            insert: true,
        }),
        "removeAt" => Ok(MutationPlan {
            index,
            remove: u32::from(index < length),
            insert: false,
        }),
        _ => Err(format!("Unsupported Array mutation {method}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_indices_and_empty_arrays_share_the_same_bounds() {
        assert_eq!(
            mutation_plan("insertAt", 3, -1).unwrap(),
            MutationPlan {
                index: 2,
                remove: 0,
                insert: true
            }
        );
        assert_eq!(mutation_plan("insertAt", 3, i32::MIN).unwrap().index, 0);
        assert_eq!(mutation_plan("insertAt", 3, i32::MAX).unwrap().index, 3);
        assert_eq!(
            mutation_plan("removeAt", 3, -1).unwrap(),
            MutationPlan {
                index: 2,
                remove: 1,
                insert: false
            }
        );
        assert_eq!(mutation_plan("removeAt", 3, -9).unwrap().index, 0);
        assert_eq!(mutation_plan("removeAt", 3, 3).unwrap().remove, 0);
        assert_eq!(mutation_plan("removeAt", 0, -1).unwrap().remove, 0);
    }

    #[test]
    fn growth_and_unknown_operations_are_rejected_before_mutation() {
        assert!(mutation_plan("insertAt", MAX_ARRAY_LENGTH, 0).is_err());
        assert!(mutation_plan("removeAt", MAX_ARRAY_LENGTH + 1, 0).is_err());
        assert!(mutation_plan("splice", 1, 0).is_err());
        assert_eq!(
            mutation_plan("removeAt", MAX_ARRAY_LENGTH, 0)
                .unwrap()
                .remove,
            1
        );
    }
}
