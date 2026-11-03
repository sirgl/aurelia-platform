//! The list of pictures a browser page is asked to fetch and decode (`web`): what the current egui
//! pass wants, in priority and screen order, and what the page has delivered. Pure bookkeeping,
//! so it is tested natively; `I` is the decoded picture (an `egui::ColorImage` in a page).
use std::collections::HashMap;

/// A picture the page's art service is to fetch and decode: a whole file or a region of an atlas
/// sheet, scaled so its longest side is `max_side` (0: as it is).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PictureRequest {
    /// Unique per file, region and size.
    pub key: String,
    /// The image file under the asset folder (`visual/generated/menu-v1.webp`).
    pub path: String,
    /// x, y, width, height of the region cut from the file, if not all of it.
    pub rect: Option<[u32; 4]>,
    pub max_side: u32,
    /// Lower is earlier: 0 for scenic heroes (the picture the eye goes to), 1 for cards.
    pub priority: u8,
}

impl PictureRequest {
    /// The request for a file (or a region of it) at a size; the key says all three.
    pub fn new(path: String, rect: Option<[u32; 4]>, max_side: u32, hero: bool) -> Self {
        Self { key: format!("{path}|{rect:?}|{max_side}"), path, rect, max_side, priority: u8::from(!hero) }
    }
}

/// Wants older than this many passes are forgotten (screens left long ago).
const FORGET_AFTER: u64 = 600;

pub struct Pictures<I> {
    /// The egui pass whose wants are the current ones.
    pass: u64,
    seq: u64,
    /// key -> (pass last wanted, order within that pass, request)
    wants: HashMap<String, (u64, u64, PictureRequest)>,
    /// Delivered, not yet taken by the loader that asked (`None`: the page has no such picture).
    results: HashMap<String, Option<I>>,
    /// The keys `requests` returned last.
    sent: Vec<String>,
}

impl<I> Default for Pictures<I> {
    fn default() -> Self {
        Self { pass: 0, seq: 0, wants: HashMap::new(), results: HashMap::new(), sent: Vec::new() }
    }
}

impl<I> Pictures<I> {
    pub fn begin_pass(&mut self, pass: u64) {
        self.pass = self.pass.max(pass);
    }

    /// The pass paints `request` and does not have it yet. The order of calls within a pass is
    /// the order on the screen, which breaks ties of equal priority.
    pub fn want(&mut self, request: &PictureRequest) {
        if self.results.contains_key(&request.key) {
            return;
        }
        let pass = self.pass;
        if let Some(entry) = self.wants.get_mut(&request.key) {
            if entry.0 == pass {
                return;
            }
            self.seq += 1;
            *entry = (pass, self.seq, request.clone());
            return;
        }
        self.seq += 1;
        self.wants.insert(request.key.clone(), (pass, self.seq, request.clone()));
    }

    pub fn deliver(&mut self, key: &str, image: Option<I>) {
        self.wants.remove(key);
        self.results.insert(key.to_owned(), image);
    }

    pub fn has(&self, key: &str) -> bool {
        self.results.contains_key(key)
    }

    pub fn take(&mut self, key: &str) -> Option<Option<I>> {
        self.results.remove(key)
    }

    /// What the current pass wants, most urgent first; `None` while that is what the last call
    /// returned (the page keeps its queue).
    pub fn requests(&mut self) -> Option<Vec<PictureRequest>> {
        let pass = self.pass;
        self.wants.retain(|_, (at, _, _)| *at + FORGET_AFTER >= pass);
        let mut current: Vec<_> = self.wants.values().filter(|(at, _, _)| *at == pass).collect();
        current.sort_by_key(|(_, seq, request)| (request.priority, *seq));
        let keys: Vec<String> = current.iter().map(|(_, _, request)| request.key.clone()).collect();
        if keys == self.sent {
            return None;
        }
        let list = current.into_iter().map(|(_, _, request)| request.clone()).collect();
        self.sent = keys;
        Some(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(name: &str) -> PictureRequest {
        PictureRequest::new(format!("visual/generated/{name}.webp"), None, 384, false)
    }

    fn keys(list: Option<Vec<PictureRequest>>) -> Vec<String> {
        list.unwrap_or_default().into_iter().map(|r| r.path.rsplit('/').next().unwrap().to_owned()).collect()
    }

    #[test]
    fn heroes_go_first_then_the_screen_order() {
        let mut p = Pictures::<()>::default();
        p.begin_pass(1);
        p.want(&card("a"));
        p.want(&PictureRequest::new("visual/generated/hero.webp".into(), None, 0, true));
        p.want(&card("b"));
        assert_eq!(keys(p.requests()), ["hero.webp", "a.webp", "b.webp"]);
    }

    #[test]
    fn an_unchanged_list_is_not_sent_twice_and_an_emptied_one_is() {
        let mut p = Pictures::<()>::default();
        p.begin_pass(1);
        p.want(&card("a"));
        assert!(p.requests().is_some());
        p.begin_pass(2);
        p.want(&card("a"));
        assert!(p.requests().is_none(), "same pictures in the same order: the page keeps its queue");
        p.begin_pass(3);
        assert_eq!(p.requests(), Some(Vec::new()), "a screen without pictures empties the page's queue");
        assert!(p.requests().is_none());
    }

    #[test]
    fn pictures_that_left_the_screen_drop_out_and_new_ones_queue_behind_the_kept() {
        let mut p = Pictures::<()>::default();
        p.begin_pass(1);
        for n in ["a", "b", "c"] {
            p.want(&card(n));
        }
        p.requests();
        p.begin_pass(2);
        p.want(&card("c"));
        p.want(&card("d"));
        assert_eq!(keys(p.requests()), ["c.webp", "d.webp"]);
    }

    #[test]
    fn a_delivered_picture_is_taken_once_and_is_not_wanted_again() {
        let mut p = Pictures::<u8>::default();
        p.begin_pass(1);
        let a = card("a");
        p.want(&a);
        p.deliver(&a.key, Some(7));
        assert!(p.has(&a.key));
        p.begin_pass(2);
        p.want(&a);
        assert_eq!(keys(p.requests()), Vec::<String>::new(), "delivered, waiting to be taken: not asked again");
        assert_eq!(p.take(&a.key), Some(Some(7)));
        assert_eq!(p.take(&a.key), None);
        p.deliver("missing", None);
        assert_eq!(p.take("missing"), Some(None), "a failure is delivered too");
    }

    #[test]
    fn the_key_names_file_region_and_size() {
        let whole = PictureRequest::new("x.webp".into(), None, 384, false);
        let region = PictureRequest::new("x.webp".into(), Some([1, 2, 3, 4]), 384, false);
        let hero = PictureRequest::new("x.webp".into(), None, 0, true);
        assert!(whole.key != region.key && whole.key != hero.key);
        assert_eq!((hero.priority, whole.priority), (0, 1));
    }

    #[test]
    fn old_wants_are_forgotten() {
        let mut p = Pictures::<()>::default();
        p.begin_pass(1);
        p.want(&card("a"));
        p.begin_pass(1 + FORGET_AFTER + 1);
        p.requests();
        assert!(p.wants.is_empty());
    }
}
