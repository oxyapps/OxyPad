//! The Edit menu's operations from Notepad++ as actions, one per menu item
//! (and per toolbar choice). Each maps to an `EditOp`, which the workspace
//! runs on the active tab (`Workspace::run_edit`).

use gpui_kit::{Context, InteractiveElement, Window, actions};
use oxypad_core::{Case, CommentMode, EditOp, Sort, Trim};

macro_rules! edit_actions {
    ($($action:ident => $op:expr,)*) => {
        actions!(oxypad, [$($action),*]);

        /// Handles every operation on `element` with `run`.
        pub fn on_edit_actions<T: 'static, E: InteractiveElement>(
            mut element: E,
            cx: &mut Context<T>,
            run: fn(&mut T, EditOp, &mut Window, &mut Context<T>),
        ) -> E {
            $(element = element.on_action(cx.listener(move |this, _: &$action, window, cx| run(this, $op, window, cx)));)*
            element
        }
    };
}

edit_actions! {
    IndentMore => EditOp::Indent { increase: true },
    IndentLess => EditOp::Indent { increase: false },

    CaseUpper => EditOp::Case(Case::Upper),
    CaseLower => EditOp::Case(Case::Lower),
    CaseProper => EditOp::Case(Case::ProperForce),
    CaseProperBlend => EditOp::Case(Case::ProperBlend),
    CaseSentence => EditOp::Case(Case::SentenceForce),
    CaseSentenceBlend => EditOp::Case(Case::SentenceBlend),
    CaseInvert => EditOp::Case(Case::Invert),
    CaseRandom => EditOp::Case(Case::Random),

    DuplicateLines => EditOp::DuplicateLines,
    RemoveDuplicateLines => EditOp::RemoveDuplicateLines { consecutive_only: false },
    RemoveConsecutiveDuplicateLines => EditOp::RemoveDuplicateLines { consecutive_only: true },
    JoinLines => EditOp::JoinLines,
    MoveLinesUp => EditOp::MoveLines { up: true },
    MoveLinesDown => EditOp::MoveLines { up: false },
    RemoveEmptyLines => EditOp::RemoveEmptyLines { including_blank: false },
    RemoveBlankLines => EditOp::RemoveEmptyLines { including_blank: true },
    InsertLineAbove => EditOp::InsertBlankLine { above: true },
    InsertLineBelow => EditOp::InsertBlankLine { above: false },
    ReverseLines => EditOp::Sort(Sort::Reverse),
    RandomizeLines => EditOp::Sort(Sort::Random),
    SortLexAscending => EditOp::Sort(Sort::LexicographicAscending),
    SortNoCaseAscending => EditOp::Sort(Sort::CaseInsensitiveAscending),
    SortLocaleAscending => EditOp::Sort(Sort::LocaleAscending),
    SortIntegerAscending => EditOp::Sort(Sort::IntegerAscending),
    SortCommaAscending => EditOp::Sort(Sort::DecimalCommaAscending),
    SortDotAscending => EditOp::Sort(Sort::DecimalDotAscending),
    SortLengthAscending => EditOp::Sort(Sort::LengthAscending),
    SortLexDescending => EditOp::Sort(Sort::LexicographicDescending),
    SortNoCaseDescending => EditOp::Sort(Sort::CaseInsensitiveDescending),
    SortLocaleDescending => EditOp::Sort(Sort::LocaleDescending),
    SortIntegerDescending => EditOp::Sort(Sort::IntegerDescending),
    SortCommaDescending => EditOp::Sort(Sort::DecimalCommaDescending),
    SortDotDescending => EditOp::Sort(Sort::DecimalDotDescending),
    SortLengthDescending => EditOp::Sort(Sort::LengthDescending),

    ToggleComment => EditOp::Comment(CommentMode::Toggle),
    LineComment => EditOp::Comment(CommentMode::Comment),
    LineUncomment => EditOp::Comment(CommentMode::Uncomment),
    BlockComment => EditOp::BlockComment,
    BlockUncomment => EditOp::BlockUncomment,

    TrimTrailing => EditOp::Trim(Trim::Trailing),
    TrimLeading => EditOp::Trim(Trim::Leading),
    TrimBoth => EditOp::Trim(Trim::Both),
    EolToSpace => EditOp::EolToSpace,
    TrimAndJoin => EditOp::TrimAndJoin,
    TabsToSpaces => EditOp::TabsToSpaces,
    SpacesToTabs => EditOp::SpacesToTabs { leading_only: false },
    LeadingSpacesToTabs => EditOp::SpacesToTabs { leading_only: true },
}
