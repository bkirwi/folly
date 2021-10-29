use armrest::ml::LanguageModel;
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub struct Dict(pub BTreeSet<String>);

const PUNCTUATION: &str = " .,\"";

impl Dict {
    const VALID: f32 = 1.0;
    // Tradeoff: you want this to be small, since any plausible input
    // is likely to do something more useful than one the game doesn't understand.
    // However! If a word is not in the dictionary, then choosing a totally
    // implausible word quite far from the input may make the recognizer seem
    // worse than it is.
    // The right value here will depend on both the quality of the model,
    // dictionary size, and some more subjective things.
    const INVALID: f32 = 0.001;

    fn contains_prefix(&self, prefix: &String) -> bool {
        self.0
            .range::<String, _>(prefix..)
            .next()
            .map_or(false, |c| c.starts_with(prefix))
    }
}

impl LanguageModel for &Dict {
    fn odds(&self, input: &str, ch: char) -> f32 {
        let Dict(words) = self;

        // TODO: use the real lexing rules from https://inform-fiction.org/zmachine/standards/z1point1/sect13.html
        if !ch.is_ascii_lowercase() && !(PUNCTUATION.contains(ch)) {
            return Dict::INVALID;
        }

        let word_start = input
            .rfind(|c| PUNCTUATION.contains(c))
            .map(|i| i + 1)
            .unwrap_or(0);

        let prefix = &input[word_start..];

        // The dictionary only has the first six characters of each word!
        if prefix.len() >= 6 {
            return Dict::VALID;
        }

        // If the current character is punctuation, we check that the prefix is a valid word
        if PUNCTUATION.contains(ch) {
            return if words.contains(prefix) || prefix.is_empty() {
                Dict::VALID
            } else {
                Dict::INVALID
            };
        }

        // Assume all numbers are valid inputs. (Names that include them normally don't put them in the dictionary.)
        // TODO: think about what happens if dictionary words contain digits.
        if ch.is_ascii_digit() {
            let starts_with_digit = prefix.chars().next().map_or(true, |c| c.is_ascii_digit());
            return if starts_with_digit {
                Dict::VALID
            } else {
                Dict::INVALID
            };
        }

        let mut prefix_string = prefix.to_string();
        if self.contains_prefix(&prefix_string) {
            prefix_string.push(ch);
            if self.contains_prefix(&prefix_string) {
                Dict::VALID
            } else {
                Dict::INVALID
            }
        } else {
            Dict::VALID
        }
    }

    fn odds_end(&self, prefix: &str) -> f32 {
        self.odds(prefix, ' ')
    }
}
