//! Undo and redo, and the grouping rule that decides how much of it one press
//! takes back.
//!
//! An [`Edit`] is a byte range and the two texts — what was there and what
//! replaced it — which is enough to apply it in either direction. A
//! [`Transaction`] is a run of them plus the selection before and after, so
//! that undoing puts the caret back where the typing started rather than where
//! it ended.
//!
//! # Grouping
//!
//! One press of `ctrl-z` should take back a word, not a letter, and should not
//! take back a word *and* the paste before it. The rule here, which is the one
//! every editor converges on:
//!
//! * A single-character insertion at the caret extends the transaction the
//!   previous one opened, as long as the previous one was also typing and ended
//!   exactly where this one starts.
//! * A backspace extends a run of backspaces the same way, backwards.
//! * A newline ends the group after itself: undo stops at line boundaries,
//!   which is what makes it usable in a script.
//! * Everything else — a paste, a caret move, an indent, a comment toggle, a
//!   find-and-replace, the commit of an IME composition — is its own
//!   transaction, and [`History::break_group`] is what the editor calls to say
//!   so.
//!
//! # Compositions
//!
//! An IME composition is not recorded while it is running. Typing `ㅎ`, `하`,
//! `한` is three replacements of the same range, and three transactions for one
//! syllable would be wrong in both directions — the intermediate states are not
//! text anyone typed. The editor opens the composition with
//! [`History::begin_composition`], which remembers where it started and what it
//! displaced, and closes it with [`History::end_composition`], which records
//! the one edit that is the difference. See [`crate::editor`] for the call
//! sites.
//!
//! # The clean state
//!
//! Whether the document is *dirty* is a question about where the history
//! stands, not about whether anything was ever typed: undoing back to the text
//! that was saved has to read clean again, and redoing away from it dirty. So
//! every transaction carries an id no other transaction in the same history
//! has, the state the document is in is named by the id on top of the undo
//! stack — or by the id of the state below the oldest one, when the stack is
//! empty — and [`History::mark_clean`] remembers that name. The document is
//! clean exactly when the history stands on it again.
//!
//! Two things keep the name honest. A transaction that is the clean state is
//! closed for good, so typing on after a save starts a group of its own rather
//! than changing what the saved name means; and a clean state that can no
//! longer be reached — it was on the redo stack when a new edit dropped that
//! stack — is forgotten, so nothing can stand on it again.
//!
//! # Limits
//!
//! Every transaction keeps both texts, so a history left to grow holds every
//! paste and every replace-all of a long session twice over. [`HistoryLimits`]
//! caps it by the number of undo steps and by the bytes of text they hold, and
//! a step that would break either cap drops the *oldest* — the newest is always
//! kept, however large, since an edit that cannot be undone at all is worse
//! than one long session's memory. Dropping the oldest moves the base: the
//! document it leaves when everything is undone is the state after the dropped
//! step, not the text that was opened. A clean state below the base is gone
//! for good, which is right — undo can no longer bring that text back — while
//! a save that landed on the dropped step itself is the new base, and so still
//! reachable.

use std::collections::VecDeque;
use std::ops::Range;

/// Where the caret and selection were.
///
/// Carried by a transaction so undo restores it; `reversed` is which end of a
/// non-empty selection the caret is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionState {
    /// The selected byte range, `start <= end`.
    pub range: Range<usize>,
    /// Whether the caret sits at `range.start` rather than at `range.end`.
    pub reversed: bool,
}

impl SelectionState {
    /// A collapsed caret at `offset`.
    pub const fn at(offset: usize) -> Self {
        Self {
            range: offset..offset,
            reversed: false,
        }
    }
}

/// One replacement, in a form that can be applied either way round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// Byte offset the replacement starts at.
    pub start: usize,
    /// What was there before.
    pub removed: String,
    /// What is there now.
    pub inserted: String,
}

impl Edit {
    /// The range this edit occupies before it is applied.
    pub fn old_range(&self) -> Range<usize> {
        self.start..self.start + self.removed.len()
    }

    /// The range this edit occupies after it is applied.
    pub fn new_range(&self) -> Range<usize> {
        self.start..self.start + self.inserted.len()
    }
}

/// What kind of change an edit was, for the grouping rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    /// A character the user typed, which may extend a run of them.
    Typing,
    /// A backspace, which may extend a run of them.
    DeleteBack,
    /// A forward delete, which may extend a run of them.
    DeleteForward,
    /// Anything else. Always its own transaction.
    Other,
}

impl Edit {
    /// The bytes of text it holds, which is what [`HistoryLimits::max_bytes`]
    /// counts.
    const fn weight(&self) -> usize {
        self.removed.len() + self.inserted.len()
    }
}

/// One undo step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    /// The edits, in the order they were applied.
    pub edits: Vec<Edit>,
    /// The selection before the first of them.
    pub before: SelectionState,
    /// The selection after the last of them.
    pub after: SelectionState,
    /// What the last edit in it was, for grouping.
    kind: EditKind,
    /// Whether another edit is still allowed to join this transaction.
    open: bool,
    /// The name of the state this transaction leaves the document in, unique
    /// within its history; see the module documentation's *The clean state*.
    id: u64,
}

impl Transaction {
    /// The bytes of text its edits hold.
    fn weight(&self) -> usize {
        self.edits.iter().map(Edit::weight).sum()
    }
}

/// How much a [`History`] keeps before it starts forgetting its oldest steps.
///
/// Both caps are checked on every recorded edit and the oldest transaction is
/// dropped while either is exceeded — except the newest, which is always kept,
/// so an edit larger than `max_bytes` on its own can still be undone once.
/// `max_transactions: 0` turns undo off altogether. See the module
/// documentation's *Limits* for what dropping does to the clean state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct HistoryLimits {
    /// The most undo steps kept.
    pub max_transactions: usize,
    /// The most bytes of text — removed and inserted, over the undo and redo
    /// stacks together — kept.
    pub max_bytes: usize,
}

impl HistoryLimits {
    /// What a new history gets: ten thousand steps, or 64 MiB of text, which
    /// is a long day of typing and a few dozen replace-alls over a large file.
    pub const DEFAULT: Self = Self::new(10_000, 64 * 1024 * 1024);

    /// No limit at all: every step is kept, which is how the history behaved
    /// before it had limits.
    pub const UNBOUNDED: Self = Self::new(usize::MAX, usize::MAX);

    /// Limits of `max_transactions` steps and `max_bytes` bytes of text.
    pub const fn new(max_transactions: usize, max_bytes: usize) -> Self {
        Self {
            max_transactions,
            max_bytes,
        }
    }
}

impl Default for HistoryLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The undo and redo stacks.
#[derive(Debug)]
pub struct History {
    /// Transactions that can be undone, oldest first. A deque because the
    /// limits drop from the front while edits arrive at the back.
    undo: VecDeque<Transaction>,
    /// Transactions that can be redone, oldest first.
    redo: Vec<Transaction>,
    /// Where a running IME composition started, and what it displaced.
    composition: Option<(usize, String, SelectionState)>,
    /// The name of the state below the oldest undoable transaction: what the
    /// document is when the undo stack is empty.
    base: u64,
    /// The id the next transaction is given. Only ever grows, which is what
    /// keeps a forgotten state from being named again.
    next_id: u64,
    /// The state [`Self::mark_clean`] last recorded, or `None` once it can no
    /// longer be reached.
    clean: Option<u64>,
    /// The bytes of text held on both stacks, kept as a running total so that
    /// checking [`HistoryLimits::max_bytes`] costs nothing per keystroke.
    bytes: usize,
    /// How much is kept; see [`HistoryLimits`].
    limits: HistoryLimits,
}

impl Default for History {
    /// An empty history standing on its clean state: a document that has not
    /// been touched has nothing to save.
    fn default() -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            composition: None,
            base: 0,
            next_id: 1,
            clean: Some(0),
            bytes: 0,
            limits: HistoryLimits::DEFAULT,
        }
    }
}

/// How large a group is allowed to grow before it is closed anyway.
///
/// A guard rather than a rule: without it, holding a key down would build one
/// transaction the length of the run, and taking a thousand characters back in
/// one press is not what the press means.
const MAX_GROUP: usize = 128;

impl History {
    /// An empty history, with [`HistoryLimits::DEFAULT`].
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty history that keeps no more than `limits`.
    pub fn with_limits(limits: HistoryLimits) -> Self {
        Self {
            limits,
            ..Self::default()
        }
    }

    /// How much this history keeps.
    pub const fn limits(&self) -> HistoryLimits {
        self.limits
    }

    /// Changes how much this history keeps, dropping the oldest steps at once
    /// if it already holds more than the new limits allow.
    pub fn set_limits(&mut self, limits: HistoryLimits) {
        self.limits = limits;
        self.trim();
    }

    /// Whether there is anything to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is anything to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Forgets everything, for [`crate::EditorView::set_text`].
    ///
    /// The state it leaves is a fresh one and the clean one: "a file was
    /// opened" is also "what is on screen is what is on disk".
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.composition = None;
        self.bytes = 0;
        self.base = self.mint();
        self.clean = Some(self.base);
    }

    /// Records the state the history stands on as the saved one.
    ///
    /// The open group is closed as well — and stays closed, since the saved
    /// transaction is never extended — so that typing on after a save is a step of its own
    /// that undo can take back to exactly what was saved. A composition still
    /// running is not part of the state: its preview is in the buffer but not
    /// in the history, so a save in the middle of one reads dirty again once
    /// the composition is recorded.
    pub fn mark_clean(&mut self) {
        self.break_group();
        self.clean = Some(self.state());
    }

    /// Whether the history stands on the state [`Self::mark_clean`] (or
    /// [`Self::clear`]) last recorded.
    ///
    /// Undo and redo move this both ways: back onto the saved state is clean
    /// again, and away from it in either direction is not. A running
    /// composition is not counted; [`crate::EditorView::is_dirty`] adds it.
    pub fn is_clean(&self) -> bool {
        self.clean == Some(self.state())
    }

    /// The name of the state the document is in: the newest undoable
    /// transaction's id, or the base when there is none.
    fn state(&self) -> u64 {
        self.undo.back().map_or(self.base, |last| last.id)
    }

    /// A transaction id no transaction in this history has had.
    fn mint(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Drops the redo stack, for an edit that makes its future unreachable —
    /// and with it the clean state, if that is where the saved text was.
    fn drop_redo(&mut self) {
        if let Some(clean) = self.clean
            && self.redo.iter().any(|transaction| transaction.id == clean)
        {
            self.clean = None;
        }
        for transaction in self.redo.drain(..) {
            self.bytes -= transaction.weight();
        }
    }

    /// Puts a new transaction on top of the undo stack and holds the limits
    /// to it.
    fn record(&mut self, transaction: Transaction) {
        self.bytes += transaction.weight();
        self.undo.push_back(transaction);
        self.trim();
    }

    /// Drops the oldest undo steps until the limits hold, moving the base up
    /// behind them; see the module documentation's *Limits*.
    fn trim(&mut self) {
        while self.undo.len() > self.limits.max_transactions
            || (self.bytes > self.limits.max_bytes && self.undo.len() > 1)
        {
            let Some(oldest) = self.undo.pop_front() else {
                break;
            };
            self.bytes -= oldest.weight();
            // The text below the dropped step is no longer reachable by undo.
            if self.clean == Some(self.base) {
                self.clean = None;
            }
            self.base = oldest.id;
        }
    }

    /// Closes the open group, so the next edit starts a transaction of its own.
    ///
    /// Called on every caret move and on every command that is not typing.
    pub fn break_group(&mut self) {
        if let Some(last) = self.undo.back_mut() {
            last.open = false;
        }
    }

    /// Records `edit`, extending the open transaction when the rule allows it.
    pub fn push(
        &mut self,
        edit: Edit,
        kind: EditKind,
        before: SelectionState,
        after: SelectionState,
    ) {
        // Any new edit invalidates the redo stack: the future it led to is no
        // longer reachable.
        self.drop_redo();

        if self.extend(&edit, kind, &after) {
            return;
        }
        // A line break closes its own group, so that undo stops at line ends
        // rather than swallowing the statement above.
        let crosses_a_line = edit.inserted.contains('\n') || edit.removed.contains('\n');
        let open = kind != EditKind::Other && !crosses_a_line;
        let id = self.mint();
        self.record(Transaction {
            edits: vec![edit],
            before,
            after,
            kind,
            open,
            id,
        });
    }

    /// Records several edits as one undo step, never folded into a group.
    ///
    /// The order matters and it is the order they were applied in: undo
    /// reverses them back to front and redo replays them front to back, which
    /// is what makes each one's offsets valid at the moment it is touched
    /// however the edits are spread over the buffer.
    pub fn push_transaction(
        &mut self,
        edits: Vec<Edit>,
        before: SelectionState,
        after: SelectionState,
    ) {
        if edits.is_empty() {
            return;
        }
        self.drop_redo();
        let id = self.mint();
        self.record(Transaction {
            edits,
            before,
            after,
            kind: EditKind::Other,
            open: false,
            id,
        });
    }

    /// Starts an IME composition at `start`, displacing `removed`.
    ///
    /// Nothing is recorded until [`Self::end_composition`]; see the module
    /// documentation for why.
    pub fn begin_composition(&mut self, start: usize, removed: String, before: SelectionState) {
        self.break_group();
        self.composition = Some((start, removed, before));
    }

    /// Whether a composition is being recorded.
    pub fn in_composition(&self) -> bool {
        self.composition.is_some()
    }

    /// Ends a composition, recording it as one edit that inserted `inserted`.
    ///
    /// A composition that inserted exactly what it displaced records nothing,
    /// which is what makes a cancelled composition invisible to undo.
    pub fn end_composition(&mut self, inserted: String, after: SelectionState) {
        let Some((start, removed, before)) = self.composition.take() else {
            return;
        };
        if removed == inserted {
            return;
        }
        self.drop_redo();
        let id = self.mint();
        self.record(Transaction {
            edits: vec![Edit {
                start,
                removed,
                inserted,
            }],
            before,
            after,
            kind: EditKind::Other,
            open: false,
            id,
        });
    }

    /// Abandons a running composition without recording anything.
    pub fn cancel_composition(&mut self) {
        self.composition = None;
    }

    /// Takes the newest transaction off the undo stack, ready to be reversed.
    ///
    /// The caller applies the reversal and calls [`Self::finish_undo`] with the
    /// same transaction.
    pub fn pop_undo(&mut self) -> Option<Transaction> {
        self.composition = None;
        let transaction = self.undo.pop_back()?;
        self.bytes -= transaction.weight();
        Some(transaction)
    }

    /// Puts a reversed transaction onto the redo stack.
    pub fn finish_undo(&mut self, transaction: Transaction) {
        self.bytes += transaction.weight();
        self.redo.push(transaction);
    }

    /// Takes the newest transaction off the redo stack, ready to be replayed.
    pub fn pop_redo(&mut self) -> Option<Transaction> {
        self.composition = None;
        let transaction = self.redo.pop()?;
        self.bytes -= transaction.weight();
        Some(transaction)
    }

    /// Puts a replayed transaction back onto the undo stack, closed.
    ///
    /// Not held to the limits again: moving a step between the stacks changes
    /// neither total, and the stack it returns to held it a moment ago.
    pub fn finish_redo(&mut self, mut transaction: Transaction) {
        transaction.open = false;
        self.bytes += transaction.weight();
        self.undo.push_back(transaction);
    }

    /// Tries to fold `edit` into the open transaction. Answers whether it did.
    fn extend(&mut self, edit: &Edit, kind: EditKind, after: &SelectionState) -> bool {
        if kind == EditKind::Other {
            return false;
        }
        let Some(last) = self.undo.back_mut() else {
            return false;
        };
        // The saved state is never extended: its id is what "clean" means, and
        // folding more typing into it would change the text behind the name.
        if !last.open || last.kind != kind || self.clean == Some(last.id) {
            return false;
        }
        let Some(previous) = last.edits.last_mut() else {
            return false;
        };
        if previous.inserted.len() + previous.removed.len() >= MAX_GROUP {
            return false;
        }

        match kind {
            EditKind::Typing => {
                // Only a straight run: the new text has to start exactly where
                // the last one ended, and neither may be a line break.
                if previous.start + previous.inserted.len() != edit.start
                    || !previous.removed.is_empty()
                    || !edit.removed.is_empty()
                    || edit.inserted.contains('\n')
                    || previous.inserted.contains('\n')
                {
                    return false;
                }
                previous.inserted.push_str(&edit.inserted);
            }
            EditKind::DeleteBack => {
                // Backspace walks left, so each edit ends where the previous
                // one began.
                if edit.start + edit.removed.len() != previous.start
                    || !edit.inserted.is_empty()
                    || !previous.inserted.is_empty()
                {
                    return false;
                }
                previous.start = edit.start;
                let mut removed = edit.removed.clone();
                removed.push_str(&previous.removed);
                previous.removed = removed;
            }
            EditKind::DeleteForward => {
                // Delete stays put and eats forwards.
                if edit.start != previous.start
                    || !edit.inserted.is_empty()
                    || !previous.inserted.is_empty()
                {
                    return false;
                }
                previous.removed.push_str(&edit.removed);
            }
            EditKind::Other => unreachable!("returned above"),
        }
        last.after = after.clone();
        // The merged edit holds exactly the new edit's text more than before.
        self.bytes += edit.weight();
        self.trim();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An insertion of `text` at `at`.
    fn typed(at: usize, text: &str) -> Edit {
        Edit {
            start: at,
            removed: String::new(),
            inserted: text.to_owned(),
        }
    }

    /// A removal of `text` at `at`.
    fn removed(at: usize, text: &str) -> Edit {
        Edit {
            start: at,
            removed: text.to_owned(),
            inserted: String::new(),
        }
    }

    /// Types `text` one character at a time, as the input handler would.
    fn type_run(history: &mut History, from: usize, text: &str) {
        let mut at = from;
        for ch in text.chars() {
            let next = at + ch.len_utf8();
            history.push(
                typed(at, &ch.to_string()),
                EditKind::Typing,
                SelectionState::at(at),
                SelectionState::at(next),
            );
            at = next;
        }
    }

    #[test]
    fn a_run_of_typing_is_one_undo_step() {
        let mut history = History::new();
        type_run(&mut history, 0, "select");
        let transaction = history.pop_undo().expect("one group");
        assert_eq!(transaction.edits, vec![typed(0, "select")]);
        assert_eq!(transaction.before, SelectionState::at(0));
        assert_eq!(transaction.after, SelectionState::at(6));
        assert!(!history.can_undo());
    }

    #[test]
    fn a_caret_move_ends_the_group() {
        let mut history = History::new();
        type_run(&mut history, 0, "sel");
        history.break_group();
        type_run(&mut history, 3, "ect");
        assert_eq!(history.undo.len(), 2);
    }

    #[test]
    fn typing_somewhere_else_ends_the_group_on_its_own() {
        let mut history = History::new();
        type_run(&mut history, 0, "abc");
        type_run(&mut history, 40, "xyz");
        assert_eq!(history.undo.len(), 2);
    }

    #[test]
    fn a_newline_closes_its_group() {
        let mut history = History::new();
        type_run(&mut history, 0, "abc");
        history.push(
            typed(3, "\n"),
            EditKind::Typing,
            SelectionState::at(3),
            SelectionState::at(4),
        );
        type_run(&mut history, 4, "def");
        assert_eq!(history.undo.len(), 3, "before, the newline, and after");
    }

    #[test]
    fn a_paste_is_never_folded_in() {
        let mut history = History::new();
        type_run(&mut history, 0, "abc");
        history.push(
            typed(3, "pasted"),
            EditKind::Other,
            SelectionState::at(3),
            SelectionState::at(9),
        );
        type_run(&mut history, 9, "def");
        assert_eq!(history.undo.len(), 3);
    }

    #[test]
    fn a_run_of_backspaces_is_one_undo_step() {
        let mut history = History::new();
        for (at, ch) in [(5, "e"), (4, "l"), (3, "e")] {
            history.push(
                removed(at, ch),
                EditKind::DeleteBack,
                SelectionState::at(at + 1),
                SelectionState::at(at),
            );
        }
        let transaction = history.pop_undo().expect("one group");
        assert_eq!(transaction.edits, vec![removed(3, "ele")]);
    }

    #[test]
    fn deleting_forwards_groups_the_other_way_round() {
        let mut history = History::new();
        for ch in ["s", "e", "l"] {
            history.push(
                removed(2, ch),
                EditKind::DeleteForward,
                SelectionState::at(2),
                SelectionState::at(2),
            );
        }
        let transaction = history.pop_undo().expect("one group");
        assert_eq!(transaction.edits, vec![removed(2, "sel")]);
    }

    #[test]
    fn a_long_run_is_broken_up() {
        let mut history = History::new();
        type_run(&mut history, 0, &"x".repeat(MAX_GROUP * 2 + 1));
        assert!(history.undo.len() > 1);
    }

    #[test]
    fn a_composition_records_one_edit_for_the_whole_syllable() {
        let mut history = History::new();
        history.begin_composition(0, String::new(), SelectionState::at(0));
        assert!(history.in_composition());
        history.end_composition("한".to_owned(), SelectionState::at(3));

        let transaction = history.pop_undo().expect("one group");
        assert_eq!(transaction.edits, vec![typed(0, "한")]);
    }

    #[test]
    fn a_composition_that_changed_nothing_records_nothing() {
        let mut history = History::new();
        history.begin_composition(0, "x".to_owned(), SelectionState::at(0));
        history.end_composition("x".to_owned(), SelectionState::at(1));
        assert!(!history.can_undo());
    }

    #[test]
    fn a_new_edit_drops_the_redo_stack() {
        let mut history = History::new();
        type_run(&mut history, 0, "abc");
        let transaction = history.pop_undo().expect("one group");
        history.finish_undo(transaction);
        assert!(history.can_redo());

        type_run(&mut history, 0, "z");
        assert!(!history.can_redo());
    }

    /// Undoes one step the way the editor does, answering whether there was one.
    fn undo(history: &mut History) -> bool {
        let Some(transaction) = history.pop_undo() else {
            return false;
        };
        history.finish_undo(transaction);
        true
    }

    /// Redoes one step the way the editor does, answering whether there was one.
    fn redo(history: &mut History) -> bool {
        let Some(transaction) = history.pop_redo() else {
            return false;
        };
        history.finish_redo(transaction);
        true
    }

    #[test]
    fn a_new_history_is_clean_and_an_edit_is_not() {
        let mut history = History::new();
        assert!(history.is_clean());
        type_run(&mut history, 0, "a");
        assert!(!history.is_clean());
        assert!(undo(&mut history));
        assert!(history.is_clean(), "undoing everything is the opened text");
    }

    #[test]
    fn undo_and_redo_move_onto_and_off_the_saved_state() {
        let mut history = History::new();
        type_run(&mut history, 0, "abc");
        history.mark_clean();
        assert!(history.is_clean());

        type_run(&mut history, 3, "def");
        assert!(!history.is_clean());
        assert!(undo(&mut history));
        assert!(history.is_clean(), "back on the save");
        assert!(undo(&mut history));
        assert!(!history.is_clean(), "before the save");
        assert!(redo(&mut history));
        assert!(history.is_clean());
        assert!(redo(&mut history));
        assert!(!history.is_clean(), "past the save");
    }

    #[test]
    fn typing_on_after_a_save_does_not_join_the_saved_group() {
        let mut history = History::new();
        type_run(&mut history, 0, "abc");
        history.mark_clean();
        type_run(&mut history, 3, "def");
        assert_eq!(history.undo.len(), 2);
    }

    #[test]
    fn a_saved_group_reopened_by_undo_is_not_extended() {
        // A paste never closes the group below it, so undoing the paste lands
        // on a group that is still open; were it extended, its name would
        // still say "clean" over text that is not what was saved.
        let mut history = History::new();
        type_run(&mut history, 0, "abc");
        history.mark_clean();
        history.push(
            typed(3, "pasted"),
            EditKind::Other,
            SelectionState::at(3),
            SelectionState::at(9),
        );
        assert!(undo(&mut history));
        assert!(history.is_clean());
        type_run(&mut history, 3, "d");
        assert!(!history.is_clean());
        assert_eq!(history.undo.len(), 2);
    }

    #[test]
    fn a_saved_state_dropped_with_the_redo_stack_is_unreachable() {
        let mut history = History::new();
        type_run(&mut history, 0, "abc");
        history.mark_clean();
        assert!(undo(&mut history));
        type_run(&mut history, 0, "x");
        assert_eq!(history.clean, None);
        assert!(undo(&mut history));
        assert!(!history.is_clean());
        assert!(redo(&mut history));
        assert!(!history.is_clean());
    }

    #[test]
    fn clearing_is_a_fresh_clean_state() {
        let mut history = History::new();
        type_run(&mut history, 0, "abc");
        history.mark_clean();
        type_run(&mut history, 3, "d");
        history.clear();
        assert!(history.is_clean());
        type_run(&mut history, 0, "e");
        assert!(!history.is_clean());
        assert!(undo(&mut history));
        assert!(history.is_clean());
    }

    #[test]
    fn a_composition_records_a_step_off_the_saved_state() {
        let mut history = History::new();
        history.mark_clean();
        history.begin_composition(0, String::new(), SelectionState::at(0));
        history.end_composition("한".to_owned(), SelectionState::at(3));
        assert!(!history.is_clean());
        assert!(undo(&mut history));
        assert!(history.is_clean());
    }

    /// A paste of `text` at `at`: always a step of its own.
    fn paste(history: &mut History, at: usize, text: &str) {
        history.push(
            typed(at, text),
            EditKind::Other,
            SelectionState::at(at),
            SelectionState::at(at + text.len()),
        );
    }

    /// The running byte total, recounted from scratch.
    fn recount(history: &History) -> usize {
        history
            .undo
            .iter()
            .chain(&history.redo)
            .map(Transaction::weight)
            .sum()
    }

    #[test]
    fn the_step_limit_drops_the_oldest() {
        let mut history = History::with_limits(HistoryLimits::new(2, usize::MAX));
        for (at, text) in [(0, "a"), (1, "b"), (2, "c")] {
            paste(&mut history, at, text);
        }
        assert_eq!(history.undo.len(), 2);
        let newest = history.pop_undo().expect("c");
        assert_eq!(newest.edits, vec![typed(2, "c")]);
        history.finish_undo(newest);
        let oldest = history.pop_undo().expect("b");
        assert_eq!(oldest.edits, vec![typed(1, "b")], "`a` was dropped");
        history.finish_undo(oldest);
        assert!(!history.can_undo());
    }

    #[test]
    fn the_byte_limit_drops_the_oldest_but_never_the_newest() {
        let mut history = History::with_limits(HistoryLimits::new(usize::MAX, 10));
        paste(&mut history, 0, "abcd");
        paste(&mut history, 4, "efgh");
        assert_eq!(history.undo.len(), 2);
        paste(&mut history, 8, "ijkl");
        assert_eq!(history.undo.len(), 2, "twelve bytes is over ten");
        assert_eq!(history.bytes, 8);

        paste(&mut history, 12, &"x".repeat(64));
        assert_eq!(history.undo.len(), 1, "an oversized step is still kept");
        assert_eq!(history.bytes, 64);
    }

    #[test]
    fn the_byte_total_follows_grouping_undo_and_redo() {
        let mut history = History::with_limits(HistoryLimits::new(usize::MAX, 200));
        type_run(&mut history, 0, &"x".repeat(MAX_GROUP * 3));
        assert_eq!(history.bytes, recount(&history));
        assert!(history.bytes <= 200);

        assert!(undo(&mut history));
        assert_eq!(history.bytes, recount(&history));
        assert!(redo(&mut history));
        assert_eq!(history.bytes, recount(&history));
        assert!(undo(&mut history));
        paste(&mut history, 0, "y");
        assert_eq!(
            history.bytes,
            recount(&history),
            "the redo stack was let go"
        );
    }

    #[test]
    fn a_lower_limit_applies_at_once() {
        let mut history = History::new();
        for at in 0..5 {
            paste(&mut history, at, "z");
        }
        history.set_limits(HistoryLimits::new(3, usize::MAX));
        assert_eq!(history.undo.len(), 3);
        assert_eq!(history.limits(), HistoryLimits::new(3, usize::MAX));
    }

    #[test]
    fn no_steps_at_all_turns_undo_off() {
        let mut history = History::with_limits(HistoryLimits::new(0, usize::MAX));
        paste(&mut history, 0, "a");
        assert!(!history.can_undo());
        assert_eq!(history.bytes, 0);
    }

    #[test]
    fn a_save_below_the_dropped_steps_can_never_be_clean_again() {
        let mut history = History::with_limits(HistoryLimits::new(2, usize::MAX));
        // Clean at the opened text, then three steps: the first is dropped,
        // and with it the only way back to what was opened.
        for (at, text) in [(0, "a"), (1, "b"), (2, "c")] {
            paste(&mut history, at, text);
        }
        assert_eq!(history.clean, None);
        while undo(&mut history) {}
        assert!(
            !history.is_clean(),
            "undo cannot bring the opened text back"
        );
    }

    #[test]
    fn a_save_on_the_dropped_step_itself_is_still_reachable() {
        let mut history = History::with_limits(HistoryLimits::new(2, usize::MAX));
        paste(&mut history, 0, "a");
        history.mark_clean();
        paste(&mut history, 1, "b");
        paste(&mut history, 2, "c");
        // `a` is gone from the stack, but the text it left is the new base,
        // and that is what was saved.
        assert_eq!(history.undo.len(), 2);
        assert!(undo(&mut history));
        assert!(!history.is_clean());
        assert!(undo(&mut history));
        assert!(!history.can_undo());
        assert!(history.is_clean());
        assert!(redo(&mut history));
        assert!(!history.is_clean());
    }
}
