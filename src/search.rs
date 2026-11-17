//! The search: what's typed in the search box, and the top bar's filters.
use crate::config::Game;

/// A search, lowercased: the name text plus its `t:` tag and `g:` genre filters.
/// "t:coop g:shoot quake" is the name "quake", tag "coop" and genre "shoot".
#[derive(Debug, Default, PartialEq)]
pub struct Query {
    pub name: String,
    pub tags: Vec<String>,
    pub genres: Vec<String>,
}

impl Query {
    pub fn parse(query: &str) -> Query {
        let mut q = Query::default();
        let mut name = Vec::new();
        for word in query.to_lowercase().split_whitespace() {
            let (list, filter) = match (word.strip_prefix("t:"), word.strip_prefix("g:")) {
                (Some(tag), _) => (&mut q.tags, tag),
                (_, Some(genre)) => (&mut q.genres, genre),
                _ => (&mut name, word),
            };
            if !filter.is_empty() {
                // a bare `t:` or `g:` is still being typed, don't filter yet
                list.push(filter.to_string());
            }
        }
        q.name = name.join(" ");
        q
    }

    pub fn matches_game(&self, g: &Game) -> bool {
        self.matches(&g.title, &g.tags, genres(g))
    }

    /// Tags match from their start, genres anywhere in them, since IGDB's have
    /// spaces ("Real Time Strategy (RTS)") and the search splits on whitespace.
    fn matches(&self, title: &str, tags: &[String], genres: &[String]) -> bool {
        title.to_lowercase().contains(&self.name)
            && self.tags.iter().all(|t| tags.iter().any(|tag| tag.to_lowercase().starts_with(t)))
            && self.genres.iter().all(|g| genres.iter().any(|genre| genre.to_lowercase().contains(g)))
    }
}

/// IGDB's genres for `g`, none until they're known.
pub fn genres(g: &Game) -> &[String] {
    g.meta.as_ref().map_or(&[], |m| &m.genres)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_splits_into_name_tags_and_genres() {
        let q = Query::parse("  T:Coop quake g:Shoot  arena t: g:");
        let strings = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(q, Query { name: "quake arena".into(), tags: strings(&["coop"]), genres: strings(&["shoot"]) });
    }

    #[test]
    fn genres_match_anywhere_in_a_genre() {
        let genres = ["Real Time Strategy (RTS)".to_string(), "Simulator".to_string()];
        let m = |q: &str| Query::parse(q).matches("Command & Conquer 3", &[], &genres);
        assert!(m("g:Strategy") && m("g:rts") && m("g:sim") && m("g:strat g:sim"));
        assert!(m("g:") && m("command g:rts"), "a bare g: doesn't filter");
        assert!(!m("g:shooter") && !m("g:rts g:shooter") && !m("quake g:rts"));
        assert!(!Query::parse("g:rts").matches("Quake", &[], &[]), "no genres known yet");
    }
}
