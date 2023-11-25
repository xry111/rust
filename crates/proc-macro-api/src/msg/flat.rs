//! Serialization-friendly representation of `tt::Subtree`.
//!
//! It is possible to serialize `Subtree` as is, as a tree, but using
//! arbitrary-nested trees in JSON is problematic, as they can cause the JSON
//! parser to overflow the stack.
//!
//! Additionally, such implementation would be pretty verbose, and we do care
//! about performance here a bit.
//!
//! So what this module does is dumping a `tt::Subtree` into a bunch of flat
//! array of numbers. See the test in the parent module to get an example
//! output.
//!
//! ```json
//!  {
//!    // Array of subtrees, each subtree is represented by 4 numbers:
//!    // id of delimiter, delimiter kind, index of first child in `token_tree`,
//!    // index of last child in `token_tree`
//!    "subtree":[4294967295,0,0,5,2,2,5,5],
//!    // 2 ints per literal: [token id, index into `text`]
//!    "literal":[4294967295,1],
//!    // 3 ints per punct: [token id, char, spacing]
//!    "punct":[4294967295,64,1],
//!    // 2 ints per ident: [token id, index into `text`]
//!    "ident":   [0,0,1,1],
//!    // children of all subtrees, concatenated. Each child is represented as `index << 2 | tag`
//!    // where tag denotes one of subtree, literal, punct or ident.
//!    "token_tree":[3,7,1,4],
//!    // Strings shared by idents and literals
//!    "text": ["struct","Foo"]
//!  }
//! ```
//!
//! We probably should replace most of the code here with bincode someday, but,
//! as we don't have bincode in Cargo.toml yet, lets stick with serde_json for
//! the time being.

use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};
use text_size::TextRange;
use tt::{Span, SyntaxContext};

use crate::msg::{ENCODE_CLOSE_SPAN_VERSION, VARIABLE_SIZED_SPANS};

pub trait SerializableSpan<const L: usize>: Span {
    fn into_u32(self) -> [u32; L];
    fn from_u32(input: [u32; L]) -> Self;
}
// impl SerializableSpan<1> for tt::TokenId {
//     fn into_u32(self) -> [u32; 1] {
//         [self.0]
//     }
//     fn from_u32([input]: [u32; 1]) -> Self {
//         tt::TokenId(input)
//     }
// }

impl<Anchor, Ctx> SerializableSpan<3> for tt::SpanData<Anchor, Ctx>
where
    Anchor: From<u32> + Into<u32>,
    Self: Span,
    Ctx: SyntaxContext,
{
    fn into_u32(self) -> [u32; 3] {
        [self.anchor.into(), self.range.start().into(), self.range.end().into()]
    }
    fn from_u32([file_id, start, end]: [u32; 3]) -> Self {
        tt::SpanData {
            anchor: file_id.into(),
            range: TextRange::new(start.into(), end.into()),
            ctx: Ctx::DUMMY,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FlatTree {
    subtree: Vec<u32>,
    literal: Vec<u32>,
    punct: Vec<u32>,
    ident: Vec<u32>,
    token_tree: Vec<u32>,
    text: Vec<String>,
    #[serde(skip_serializing_if = "SpanMap::do_serialize")]
    #[serde(default)]
    span_map: SpanMap,
}

#[derive(Serialize, Deserialize, Debug)]
struct SpanMap {
    #[serde(skip_serializing)]
    serialize: bool,
    span_size: u32,
    spans: Vec<u32>,
}

impl Default for SpanMap {
    fn default() -> Self {
        Self { serialize: false, span_size: 1, spans: Default::default() }
    }
}

impl SpanMap {
    fn serialize_span<const L: usize, S: SerializableSpan<L>>(&mut self, span: S) -> u32 {
        let u32s = span.into_u32();
        if L == 1 {
            u32s[0]
        } else {
            let offset = self.spans.len() as u32;
            self.spans.extend(u32s);
            offset
        }
    }
    fn deserialize_span<const L: usize, S: SerializableSpan<L>>(&self, offset: u32) -> S {
        S::from_u32(if L == 1 {
            [offset].as_ref().try_into().unwrap()
        } else {
            self.spans[offset as usize..][..L].try_into().unwrap()
        })
    }
}

impl SpanMap {
    fn do_serialize(&self) -> bool {
        self.serialize
    }
}

struct SubtreeRepr<const L: usize, S> {
    open: S,
    close: S,
    kind: tt::DelimiterKind,
    tt: [u32; 2],
}

struct LiteralRepr<const L: usize, S> {
    id: S,
    text: u32,
}

struct PunctRepr<const L: usize, S> {
    id: S,
    char: char,
    spacing: tt::Spacing,
}

struct IdentRepr<const L: usize, S> {
    id: S,
    text: u32,
}

impl FlatTree {
    pub fn new<const L: usize, S: SerializableSpan<L>>(
        subtree: &tt::Subtree<S>,
        version: u32,
    ) -> FlatTree {
        let mut w = Writer {
            string_table: HashMap::new(),
            work: VecDeque::new(),

            subtree: Vec::new(),
            literal: Vec::new(),
            punct: Vec::new(),
            ident: Vec::new(),
            token_tree: Vec::new(),
            text: Vec::new(),
        };
        w.write(subtree);
        assert!(L == 1 || version >= VARIABLE_SIZED_SPANS);
        let mut span_map = SpanMap {
            serialize: version >= VARIABLE_SIZED_SPANS && L != 1,
            span_size: L as u32,
            spans: Vec::new(),
        };
        return FlatTree {
            subtree: if version >= ENCODE_CLOSE_SPAN_VERSION {
                write_vec(&mut span_map, w.subtree, SubtreeRepr::write_with_close_span)
            } else {
                write_vec(&mut span_map, w.subtree, SubtreeRepr::write)
            },
            literal: write_vec(&mut span_map, w.literal, LiteralRepr::write),
            punct: write_vec(&mut span_map, w.punct, PunctRepr::write),
            ident: write_vec(&mut span_map, w.ident, IdentRepr::write),
            token_tree: w.token_tree,
            text: w.text,
            span_map,
        };

        fn write_vec<T, F: Fn(T, &mut SpanMap) -> [u32; N], const N: usize>(
            map: &mut SpanMap,
            xs: Vec<T>,
            f: F,
        ) -> Vec<u32> {
            xs.into_iter().flat_map(|it| f(it, map)).collect()
        }
    }

    pub fn to_subtree<const L: usize, S: SerializableSpan<L>>(
        self,
        version: u32,
    ) -> tt::Subtree<S> {
        assert!((version >= VARIABLE_SIZED_SPANS || L == 1) && L as u32 == self.span_map.span_size);
        return Reader {
            subtree: if version >= ENCODE_CLOSE_SPAN_VERSION {
                read_vec(&self.span_map, self.subtree, SubtreeRepr::read_with_close_span)
            } else {
                read_vec(&self.span_map, self.subtree, SubtreeRepr::read)
            },
            literal: read_vec(&self.span_map, self.literal, LiteralRepr::read),
            punct: read_vec(&self.span_map, self.punct, PunctRepr::read),
            ident: read_vec(&self.span_map, self.ident, IdentRepr::read),
            token_tree: self.token_tree,
            text: self.text,
        }
        .read();

        fn read_vec<T, F: Fn([u32; N], &SpanMap) -> T, const N: usize>(
            map: &SpanMap,
            xs: Vec<u32>,
            f: F,
        ) -> Vec<T> {
            let mut chunks = xs.chunks_exact(N);
            let res = chunks.by_ref().map(|chunk| f(chunk.try_into().unwrap(), map)).collect();
            assert!(chunks.remainder().is_empty());
            res
        }
    }
}

impl<const L: usize, S: SerializableSpan<L>> SubtreeRepr<L, S> {
    fn write(self, map: &mut SpanMap) -> [u32; 4] {
        let kind = match self.kind {
            tt::DelimiterKind::Invisible => 0,
            tt::DelimiterKind::Parenthesis => 1,
            tt::DelimiterKind::Brace => 2,
            tt::DelimiterKind::Bracket => 3,
        };
        [map.serialize_span(self.open), kind, self.tt[0], self.tt[1]]
    }
    fn read([open, kind, lo, len]: [u32; 4], map: &SpanMap) -> Self {
        let kind = match kind {
            0 => tt::DelimiterKind::Invisible,
            1 => tt::DelimiterKind::Parenthesis,
            2 => tt::DelimiterKind::Brace,
            3 => tt::DelimiterKind::Bracket,
            other => panic!("bad kind {other}"),
        };
        SubtreeRepr { open: map.deserialize_span(open), close: S::DUMMY, kind, tt: [lo, len] }
    }
    fn write_with_close_span(self, map: &mut SpanMap) -> [u32; 5] {
        let kind = match self.kind {
            tt::DelimiterKind::Invisible => 0,
            tt::DelimiterKind::Parenthesis => 1,
            tt::DelimiterKind::Brace => 2,
            tt::DelimiterKind::Bracket => 3,
        };
        [
            map.serialize_span(self.open),
            map.serialize_span(self.close),
            kind,
            self.tt[0],
            self.tt[1],
        ]
    }
    fn read_with_close_span([open, close, kind, lo, len]: [u32; 5], map: &SpanMap) -> Self {
        let kind = match kind {
            0 => tt::DelimiterKind::Invisible,
            1 => tt::DelimiterKind::Parenthesis,
            2 => tt::DelimiterKind::Brace,
            3 => tt::DelimiterKind::Bracket,
            other => panic!("bad kind {other}"),
        };
        SubtreeRepr {
            open: map.deserialize_span(open),
            close: map.deserialize_span(close),
            kind,
            tt: [lo, len],
        }
    }
}

impl<const L: usize, S: SerializableSpan<L>> LiteralRepr<L, S> {
    fn write(self, map: &mut SpanMap) -> [u32; 2] {
        [map.serialize_span(self.id), self.text]
    }
    fn read([id, text]: [u32; 2], map: &SpanMap) -> Self {
        LiteralRepr { id: map.deserialize_span(id), text }
    }
}

impl<const L: usize, S: SerializableSpan<L>> PunctRepr<L, S> {
    fn write(self, map: &mut SpanMap) -> [u32; 3] {
        let spacing = match self.spacing {
            tt::Spacing::Alone => 0,
            tt::Spacing::Joint => 1,
        };
        [map.serialize_span(self.id), self.char as u32, spacing]
    }
    fn read([id, char, spacing]: [u32; 3], map: &SpanMap) -> Self {
        let spacing = match spacing {
            0 => tt::Spacing::Alone,
            1 => tt::Spacing::Joint,
            other => panic!("bad spacing {other}"),
        };
        PunctRepr { id: map.deserialize_span(id), char: char.try_into().unwrap(), spacing }
    }
}

impl<const L: usize, S: SerializableSpan<L>> IdentRepr<L, S> {
    fn write(self, map: &mut SpanMap) -> [u32; 2] {
        [map.serialize_span(self.id), self.text]
    }
    fn read(data: [u32; 2], map: &SpanMap) -> Self {
        IdentRepr { id: map.deserialize_span(data[0]), text: data[1] }
    }
}

struct Writer<'a, const L: usize, S> {
    work: VecDeque<(usize, &'a tt::Subtree<S>)>,
    string_table: HashMap<&'a str, u32>,

    subtree: Vec<SubtreeRepr<L, S>>,
    literal: Vec<LiteralRepr<L, S>>,
    punct: Vec<PunctRepr<L, S>>,
    ident: Vec<IdentRepr<L, S>>,
    token_tree: Vec<u32>,
    text: Vec<String>,
}

impl<'a, const L: usize, S: Copy> Writer<'a, L, S> {
    fn write(&mut self, root: &'a tt::Subtree<S>) {
        self.enqueue(root);
        while let Some((idx, subtree)) = self.work.pop_front() {
            self.subtree(idx, subtree);
        }
    }

    fn subtree(&mut self, idx: usize, subtree: &'a tt::Subtree<S>) {
        let mut first_tt = self.token_tree.len();
        let n_tt = subtree.token_trees.len();
        self.token_tree.resize(first_tt + n_tt, !0);

        self.subtree[idx].tt = [first_tt as u32, (first_tt + n_tt) as u32];

        for child in &subtree.token_trees {
            let idx_tag = match child {
                tt::TokenTree::Subtree(it) => {
                    let idx = self.enqueue(it);
                    idx << 2
                }
                tt::TokenTree::Leaf(leaf) => match leaf {
                    tt::Leaf::Literal(lit) => {
                        let idx = self.literal.len() as u32;
                        let text = self.intern(&lit.text);
                        self.literal.push(LiteralRepr { id: lit.span, text });
                        idx << 2 | 0b01
                    }
                    tt::Leaf::Punct(punct) => {
                        let idx = self.punct.len() as u32;
                        self.punct.push(PunctRepr {
                            char: punct.char,
                            spacing: punct.spacing,
                            id: punct.span,
                        });
                        idx << 2 | 0b10
                    }
                    tt::Leaf::Ident(ident) => {
                        let idx = self.ident.len() as u32;
                        let text = self.intern(&ident.text);
                        self.ident.push(IdentRepr { id: ident.span, text });
                        idx << 2 | 0b11
                    }
                },
            };
            self.token_tree[first_tt] = idx_tag;
            first_tt += 1;
        }
    }

    fn enqueue(&mut self, subtree: &'a tt::Subtree<S>) -> u32 {
        let idx = self.subtree.len();
        let open = subtree.delimiter.open;
        let close = subtree.delimiter.close;
        let delimiter_kind = subtree.delimiter.kind;
        self.subtree.push(SubtreeRepr { open, close, kind: delimiter_kind, tt: [!0, !0] });
        self.work.push_back((idx, subtree));
        idx as u32
    }

    pub(crate) fn intern(&mut self, text: &'a str) -> u32 {
        let table = &mut self.text;
        *self.string_table.entry(text).or_insert_with(|| {
            let idx = table.len();
            table.push(text.to_string());
            idx as u32
        })
    }
}

struct Reader<const L: usize, S> {
    subtree: Vec<SubtreeRepr<L, S>>,
    literal: Vec<LiteralRepr<L, S>>,
    punct: Vec<PunctRepr<L, S>>,
    ident: Vec<IdentRepr<L, S>>,
    token_tree: Vec<u32>,
    text: Vec<String>,
}

impl<const L: usize, S: SerializableSpan<L>> Reader<L, S> {
    pub(crate) fn read(self) -> tt::Subtree<S> {
        let mut res: Vec<Option<tt::Subtree<S>>> = vec![None; self.subtree.len()];
        for i in (0..self.subtree.len()).rev() {
            let repr = &self.subtree[i];
            let token_trees = &self.token_tree[repr.tt[0] as usize..repr.tt[1] as usize];
            let s = tt::Subtree {
                delimiter: tt::Delimiter { open: repr.open, close: repr.close, kind: repr.kind },
                token_trees: token_trees
                    .iter()
                    .copied()
                    .map(|idx_tag| {
                        let tag = idx_tag & 0b11;
                        let idx = (idx_tag >> 2) as usize;
                        match tag {
                            // XXX: we iterate subtrees in reverse to guarantee
                            // that this unwrap doesn't fire.
                            0b00 => res[idx].take().unwrap().into(),
                            0b01 => {
                                let repr = &self.literal[idx];
                                tt::Leaf::Literal(tt::Literal {
                                    text: self.text[repr.text as usize].as_str().into(),
                                    span: repr.id,
                                })
                                .into()
                            }
                            0b10 => {
                                let repr = &self.punct[idx];
                                tt::Leaf::Punct(tt::Punct {
                                    char: repr.char,
                                    spacing: repr.spacing,
                                    span: repr.id,
                                })
                                .into()
                            }
                            0b11 => {
                                let repr = &self.ident[idx];
                                tt::Leaf::Ident(tt::Ident {
                                    text: self.text[repr.text as usize].as_str().into(),
                                    span: repr.id,
                                })
                                .into()
                            }
                            other => panic!("bad tag: {other}"),
                        }
                    })
                    .collect(),
            };
            res[i] = Some(s);
        }

        res[0].take().unwrap()
    }
}
