//! The form-script lexer, parser and evaluator.
//!
//! A calculate action's source is attacker-controlled text that arrives inside
//! an otherwise sound file, so it is the same class of input as a content
//! stream and gets the same treatment: never a panic, never an unbounded run
//! (ruling 1).
//!
//! Two properties are checked rather than one. The first is the usual
//! no-panic. The second is the one this target exists for: **the budget is
//! honoured**. A run is given a small budget and must come back having spent
//! no more than it, so a path that forgot to charge a step — which is how a
//! bounded interpreter stops being bounded — fails the target instead of
//! merely running slowly and being blamed on the corpus.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_cos::{Budget, Host, ScriptError};

/// A handful of fields, so `getField` and `AFSimple_Calculate` have somewhere
/// to land and the evaluator is exercised past the parser.
struct Fields {
    values: Vec<(&'static str, String)>,
}

impl Host for Fields {
    fn field(&self, name: &str) -> Option<String> {
        self.values
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.clone())
    }

    fn set_field(&mut self, name: &str, value: &str) -> bool {
        match self.values.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => {
                slot.1 = value.to_string();
                true
            }
            None => false,
        }
    }
}

fuzz_target!(|data: &[u8]| {
    // Source is text; invalid UTF-8 is the document's problem and lossy
    // decoding is what the field model does with a `/JS` string too.
    let source = String::from_utf8_lossy(data);

    let mut host = Fields {
        values: vec![
            ("a", "10".to_string()),
            ("b", "-2.5".to_string()),
            ("c", String::new()),
            ("total", "0".to_string()),
        ],
    };

    // Deliberately small. The point is not to let a script finish, it is to
    // prove the cap holds whatever the script does.
    const LIMIT: u32 = 5_000;
    let mut budget = Budget::new(LIMIT);
    let outcome = tinker_pdf_cos::script::run(&source, "total", "1234.5", &mut host, &mut budget);

    assert!(
        budget.used() <= LIMIT,
        "the budget was overrun: {} of {LIMIT}",
        budget.used()
    );
    if let Err(ScriptError::OutOfSteps) = outcome {
        assert!(budget.is_spent(), "OutOfSteps with budget left");
    }
});
