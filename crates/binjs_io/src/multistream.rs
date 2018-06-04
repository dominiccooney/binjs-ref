//! A format in which we split everything per nature.
//!
//! - One stream of node kinds
//! - One stream of strings
//! - One stream of floats
//! - ...
//!
//! Each stream is compressed separately, which should increase compressibility.

// FIXME: Can we easily interleave streams of e.g. gzip, brotli?

// FIXME: Since we're going to inline the definition of node kinds, we could try and toy with
// with the bytes used to represent child instances to make them use the same alphabet.

use io::TokenWriter;
use labels::{ Dictionary, Label as WritableLabel };
use ::{ DictionaryPlacement, CompressionTarget, TokenWriterError };
use util:: { Counter, GenericCounter };

use std;
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{ Hash, Hasher };
use std::io::Write;
use std::rc::Rc;

use itertools::Itertools;

#[derive(Clone, Debug)]
pub struct Options {
    pub sibling_labels_together: bool,
    pub dictionary_placement: DictionaryPlacement,
}
#[derive(Clone)]
pub struct Targets {
    pub contents: PerCategory<CompressionTarget>,
    pub header_identifiers: CompressionTarget,
    pub header_strings: CompressionTarget,
    pub header_tags: CompressionTarget,
}
impl Targets {
    pub fn reset(&mut self) {
        self.contents.reset();
        self.header_identifiers.reset();
        self.header_strings.reset();
        self.header_tags.reset();
    }
}

#[derive(Clone, Debug, Default)]
pub struct PerCategory<T> {
    pub declarations: T,
    pub idrefs: T,
    pub strings: T,
    pub numbers: T,
    pub bools: T,
    pub lists: T,
    pub tags: T,
}
impl PerCategory<CompressionTarget> {
    pub fn reset(&mut self) {
        self.declarations.reset();
        self.idrefs.reset();
        self.strings.reset();
        self.numbers.reset();
        self.bools.reset();
        self.lists.reset();
        self.tags.reset();
    }
}

impl std::ops::Add<Self> for PerCategory<usize> {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            strings: self.strings + other.strings,
            numbers: self.numbers + other.numbers,
            bools: self.bools + other.bools,
            lists: self.lists + other.lists,
            tags: self.tags + other.tags,
            declarations: self.declarations + other.declarations,
            idrefs: self.idrefs + other.idrefs,
        }
    }
}
impl std::fmt::Display for PerCategory<usize> {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(fmt, "declarations (b): {declarations}, idrefs (b): {idrefs}, strings (b): {strings}, tags (b): {tags}, numbers (b): {numbers}, bools (b): {bools}, lists (b): {lists}",
            declarations = self.declarations,
            idrefs = self.idrefs,
            strings = self.strings,
            numbers = self.numbers,
            bools = self.bools,
            lists = self.lists,
            tags = self.tags,
        )
    }
}

struct Compressor<W: Write + Sized> {
    stream: W,
    dictionary: Box<Dictionary<Label, W>>,
}

#[derive(Debug, Default)]
pub struct Statistics {
    pub header_strings: usize,
    pub header_tags: usize,
    pub contents: PerCategory<usize>,
}
impl std::ops::Add<Self> for Statistics {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            header_strings: self.header_strings + other.header_strings,
            header_tags: self.header_tags + other.header_tags,
            contents: self.contents + other.contents,
        }
    }
}

impl std::fmt::Display for Statistics {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(fmt, "Header strings (b): {strings}, header tags (b): {tags}, {rest}",
            strings = self.header_strings,
            tags = self.header_tags,
            rest = self.contents,
        )
    }
}

#[derive(Clone, Debug)]
pub struct SubTree {
    label: Label,
    children: Vec<SharedTree>,
}
pub type SharedTree = Rc<RefCell<SubTree>>;

#[derive(Clone, Copy, Debug)]
enum Direction {
    Enter,
    Exit
}

impl SubTree {
    fn with_labels<F: FnMut(&Label)>(&self, f: &mut F) {
        f(&self.label);
        for child in &self.children {
            child.borrow().with_labels(f);
        }
    }
    fn with_labels_mut<F: FnMut(Direction, &mut Label)>(&mut self, f: &mut F) {
        f(Direction::Enter, &mut self.label);
        for child in &self.children {
            child.borrow_mut().with_labels_mut(f);
        }
        f(Direction::Exit, &mut self.label);
    }
    fn serialize_label<W: Write>(&self, parent: Option<&Label>, compressors: &mut PerCategory<Compressor<W>>) -> Result<(), std::io::Error> {
        let compressor = match self.label {
            Label::String(_) => &mut compressors.strings,
            Label::Number(_) => &mut compressors.numbers,
            Label::Bool(_)   => &mut compressors.bools,
            Label::List(_)   => &mut compressors.lists,
            Label::Tag(_)    => &mut compressors.tags,
            Label::Declare(_) => &mut compressors.declarations,
            Label::NumberedReference(n) => &mut compressors.idrefs,
            Label::Scope(_) => /* Nothing to do */ return Ok(()),
            _ => unimplemented!()
        };
        compressor.dictionary.write_label(&self.label, parent, &mut compressor.stream)?;
        Ok(())
    }
    fn serialize_children<W: Write>(&self, options: &Options, parent: Option<&Label>, compressors: &mut PerCategory<Compressor<W>>) -> Result<(), std::io::Error> {
        let new_parent = match self.label {
            Label::Tag(_) => Some(&self.label),
            _ => parent
        };
        if options.sibling_labels_together {
            // First all the labels of children.
            for child in &self.children {
                let borrow = child.borrow();
                borrow.serialize_label(new_parent, compressors)?;
            }
            // Then actually walk the children.
            for child in &self.children {
                let borrow = child.borrow();
                borrow.serialize_children(options, new_parent, compressors)?;
            }
        } else {
            // Everything at once.
            for child in &self.children {
                let borrow = child.borrow();
                borrow.serialize_label(new_parent, compressors)?;
                borrow.serialize_children(options, new_parent, compressors)?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ScopeIndex(usize);
impl Counter for ScopeIndex {
    fn internal_make(value: usize) -> Self {
        ScopeIndex(value)
    }
}

/// A trivial wrapping of f64 with Hash and Eq.
#[derive(Clone, Debug, PartialEq, Copy)]
pub struct F64(pub f64);
impl Eq for F64 { } // Not strictly true.
impl Hash for F64 {
    fn hash<H>(&self, state: &mut H) where H: Hasher {
        self.0.to_bits().hash(state)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Label {
    String(Option<Rc<String>>),
    Number(Option<F64>),
    Bool(Option<bool>),
    List(Option<u32>),
    Tag(Rc<String>),
    /// Scope. Any `Declare` within the `Scope`
    /// stays in the `Scope`.
    Scope(ScopeIndex),
    /// Declare a variable throughout the current `Scope`.
    Declare(Option<Rc<String>>),
    /// Reference a variable throughout the current `Scope`.
    ///
    /// Initially entered as `LiteralReference`, then processed to a `NumberedReference`.
    LiteralReference(Option<Rc<String>>),
    NumberedReference(Option<u32>),
}
impl Label {
    // FIXME: Make this more robust.
    fn string_byte_len(&self) -> usize {
        let result = match *self {
            Label::String(None) // FIXME: We could turn this into `0`, if we wanted. Not really necessary, though.
            | Label::Declare(None)
            | Label::LiteralReference(None) => 2,
            Label::String(Some(ref string))
            | Label::Declare(Some(ref string))
            | Label::LiteralReference(Some(ref string)) => string.len(),
            Label::Tag(ref string) => string.len(),
            _ => panic!("string_byte_len not implemented for this case {:?}", self),
        };
        result
    }
}

impl WritableLabel for Label {
    fn write_definition<W: Write, L: Dictionary<Self, W>>(&self, index: Option<usize>, _parent: Option<&Self>, _strategy: &mut L, out: &mut W) -> Result<(), std::io::Error> {
        use self::Label::*;
        if let Some(index) = index {
            use bytes::varnum::WriteVarNum;
            out.write_varnum(index as u32)?;
        }
        match *self {
            String(Some(ref s))
            | Declare(Some(ref s)) => {
                out.write_all(s.as_bytes())?;
            },
            String(None)
            | Declare(None) => {
              // FIXME: Put this magic constant safely in a module
              out.write_all(&[255, 0])?;
            },
            Number(maybe_num) => {
              out.write_all(&::bytes::float::varbytes_of_float(maybe_num.map(|x| x.0)))?;
            }
            Bool(maybe_bool) => {
              out.write_all(&::bytes::bool::bytes_of_bool(maybe_bool))?;
            }
            List(maybe_len) => {
                use ::bytes::varnum::*;
                out.write_maybe_varnum(maybe_len)?;
            }
            Tag(ref s) => {
                out.write_all(s.as_bytes())?;
            }
            NumberedReference(maybe_index) => {
                use ::bytes::varnum::*;
                out.write_maybe_varnum(maybe_index)?;
            }
            _ => unimplemented!("{:?}", self)
        }
        Ok(())
    }
}

pub struct TreeTokenWriter {
    root: SharedTree,
    options: Options,
    targets: Targets,
    scope_counter: GenericCounter<ScopeIndex>,
}
impl TreeTokenWriter {
    pub fn new(options: Options, targets: Targets) -> Self {
        Self {
            options,
            targets,
            scope_counter: GenericCounter::new(),
            root: Rc::new(RefCell::new(SubTree {
                label: Label::String(None),
                children: vec![]
            }))
        }
    }
    fn new_tree(&mut self, tree: SubTree) -> Result<SharedTree, TokenWriterError> {
        self.root = Rc::new(RefCell::new(tree));
        Ok(self.root.clone())
    }

    fn number_references(&mut self) -> Result<SharedTree, TokenWriterError> {
        // Undeclared references
        let top = Rc::new(RefCell::new(vec![]));
        let stack = Rc::new(RefCell::new(vec![top.clone()]));
        self.root.borrow_mut().with_labels_mut(&mut |direction, label| {
            let rewrite = match (direction, &label) {
                (Direction::Enter, Label::Scope(_)) => {
                    let mut borrow_stack = stack.borrow_mut();
                    borrow_stack.push(Rc::new(RefCell::new(vec![])));
                    None
                }
                (Direction::Exit, Label::Scope(_)) => {
                    let mut borrow_stack = stack.borrow_mut();
                    borrow_stack.pop();
                    None
                }
                (Direction::Enter, Label::Declare(Some(ref s))) => {
                    let borrow_stack = stack.borrow();
                    let mut borrow_frame = borrow_stack.last()
                        .unwrap()
                        .borrow_mut();
                    borrow_frame
                        .push(s.clone());
                    None
                }
                (Direction::Enter, Label::LiteralReference(None)) => {
                    Some(Label::NumberedReference(None))
                }
                (Direction::Enter, Label::LiteralReference(Some(ref s))) => {
                    let mut depth = 0;
                    let mut found = None;
                    {
                        let borrow_stack = stack.borrow();
                        'find_in_stack: for frame in borrow_stack.iter().rev() {
                            let borrow_frame = frame.borrow();
                            if let Some(index) = borrow_frame.iter()
                                .position(|name| name == s)
                            {
                                found = Some(index);
                                break 'find_in_stack;
                            } else {
                                depth += borrow_frame.len()
                            }
                        }
                    }
                    let index = match found {
                        Some(found) => found,
                        None => {
                            let mut borrow_top = top.borrow_mut();
                            borrow_top.push(s.clone());
                            borrow_top.len()
                        }
                    };
                    Some(Label::NumberedReference(Some((depth + index) as u32)))
                }
                _ => None,
            };
            if let Some(rewrite) = rewrite {
                *label = rewrite;
            }
        });

        // Now declare all undeclared variables.
        let root = self.root.clone();
        let top = top.borrow()
            .iter()
            .map(|name| Rc::new(RefCell::new(SubTree {
                label: Label::Declare(Some(name.clone())),
                children: vec![]
            })))
            .collect();
        let declared_undeclared_variables = self.new_tree(SubTree {
            label: Label::Tag(Rc::new("_undeclared_variables".to_string())),
            children: top
        })?;
        let scope_index = self.scope_counter.next();
        self.new_tree(SubTree {
            label: Label::Scope(scope_index),
            children: vec![
                declared_undeclared_variables,
                root
            ]
        })
    }
}

    // Creates a string table ordered for effective compression later.
//
// String references are encoded using three most recently used registers:
//
// Byte 0, l.o. bits:  h.o. bits:
//   00                -2^5 .. 2^5-1 two's complement delta of MRU register 0
//   01                -2^5 .. 2^5-1 two's complement delta of MRU register 1
//   10                -2^5 .. 2^5-1 two's complement delta of MRU register 2
//   11                literal; if bit 3 is set, continues in subsequent bytes
//
// When a multi-byte literal is mentioned (.....111) the least
// recently used register is evicted, registers shuffle down, and
// MRU-0 is set to the literal.
//
// When a register delta is mentioned (00, 01, 10) *that* register is
// evicted, the registers shuffle down (if 01, 10) and MRU-0 is set to
// the old register value with the delta applied. This preserves
// diversity of the MRU values.
//
// The 5-bit literal nnnnn011 fits in a single byte, so the strings
// are organized this way:
//
// - The top 2^5 most frequent strings are assigned indices 0 ..
//   2^5-1. These strings may account for a third or more of the
//   references.
//
// - The less frequent strings are assigned indices 2^5 ..
//
// - To induce Brotli copies, strings within each group are sorted
//   lexicographically (although other sortings are allowable.)
//
// - To induce Brotli copies between the groups, the frequent strings
//   are sorted in reverse lexicographic order (although other
//   sortings are allowable.)
fn lay_out_string_table(string_instances: &mut HashMap<Label, usize>) -> Vec<Label> {
    let (top_strings, bottom_strings): (Vec<(usize, &Label)>, Vec<(usize, &Label)>) = string_instances
        .into_iter()
        .sorted_by(|a,b| usize::cmp(&b.1, &a.1))
        .into_iter()
        .map(|(string, occurrence)| string)
        .enumerate()
        .partition(|(i, string)| *i < 32);

    top_strings
        .into_iter()
        .map(|(i, string)| string.clone()) // strip off the indices
        .sorted_by(|a, b| compare_string_label_lexicographically(&b, &a))
        .into_iter()
//        .inspect(|string| debug!(target: "multistream", "top string: {:?}", string))
        .chain(
            bottom_strings
                .into_iter()
                .map(|(i, string)| string.clone()) // strip off the indices
                .sorted_by(|a, b| compare_string_label_lexicographically(&a, &b))
                .into_iter()
            //                .inspect(|string| debug!(target: "multistream", "bottom string: {:?}", string))
        )
        .collect()
/*
    by_frequency_desc
        .into_iter()
        .sorted_by(|a,b| compare_string_label_lexicographically(a, b))
        .into_iter()
        .map(|s| s.clone())
        .inspect(|x| debug!(target: "multistream", "sorted: {:?}", x))
        .collect()
*/
}

fn compare_string_label_lexicographically(a: &Label, b: &Label) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Label::String(None), Label::String(None)) => { Ordering::Equal }
        (Label::String(None), _) => { Ordering::Less }
        (_, Label::String(None)) => { Ordering::Greater }
        (Label::String(Some(p)), Label::String(Some(q))) => { str::cmp(&p, &q) }
        _ => panic!("string frequencies contains non-strings")
    }
}

impl TokenWriter for TreeTokenWriter {
    type Error = TokenWriterError;
    type Statistics = Statistics;
    type Tree = SharedTree;
    type Data = Vec<u8>;

    fn tagged_tuple(&mut self, tag: &str, children: &[(&str, Self::Tree)]) -> Result<Self::Tree, Self::Error> {
        self.new_tree(SubTree {
            label: Label::Tag(Rc::new(tag.to_string())),
            children: children.iter()
                .map(|(_, tree)| tree.clone())
                .collect()
        })
    }

    fn offset(&mut self) -> Result<Self::Tree, Self::Error> {
        unimplemented!()
    }

    fn bool(&mut self, value: Option<bool>) -> Result<Self::Tree, Self::Error> {
        self.new_tree(SubTree {
            label: Label::Bool(value),
            children: vec![]
        })
    }

    fn float(&mut self, value: Option<f64>) -> Result<Self::Tree, Self::Error> {
        self.new_tree(SubTree {
            label: Label::Number(value.map(F64)),
            children: vec![]
        })
    }

    fn string(&mut self, value: Option<&str>) -> Result<Self::Tree, Self::Error> {
        let value = match (value, &self.options.dictionary_placement) {
            (None, _) => None,
            (Some(ref s), &DictionaryPlacement::Inline) => {
                let mut string = s.to_string();
                string.push('\0');
                Some(string)
            }
            (Some(ref s), _) => Some(s.to_string())
        };

        self.new_tree(SubTree {
            label: Label::String(value.map(Rc::new)),
            children: vec![]
        })
    }

    fn identifier_definition(&mut self, value: Option<&str>) -> Result<Self::Tree, Self::Error> {
        let value = match (value, &self.options.dictionary_placement) {
            (None, _) => None,
            (Some(ref s), &DictionaryPlacement::Inline) => {
                let mut string = s.to_string();
                string.push('\0');
                Some(string)
            }
            (Some(ref s), _) => Some(s.to_string())
        };

        self.new_tree(SubTree {
            label: Label::Declare(value.map(Rc::new)),
            children: vec![]
        })
    }

    fn identifier_reference(&mut self, value: Option<&str>) -> Result<Self::Tree, Self::Error> {
        let value = match (value, &self.options.dictionary_placement) {
            (None, _) => None,
            (Some(ref s), &DictionaryPlacement::Inline) => {
                let mut string = s.to_string();
                string.push('\0');
                Some(string)
            }
            (Some(ref s), _) => Some(s.to_string())
        };

        self.new_tree(SubTree {
            label: Label::LiteralReference(value.map(Rc::new)),
            children: vec![]
        })
    }

    fn tagged_scope_tuple(&mut self, tag: &str, children: &[(&str, Self::Tree)]) -> Result<Self::Tree, Self::Error> {
        let tuple = self.tagged_tuple(tag, children)?;
        let scope_index = self.scope_counter.next();
        self.new_tree(SubTree {
            label: Label::Scope(scope_index),
            children: vec![tuple]
        })
    }

    fn list(&mut self, children: Vec<Self::Tree>) -> Result<Self::Tree, Self::Error> {
        self.new_tree(SubTree {
            label: Label::List(Some(children.len() as u32)),
            children
        })
    }

    fn untagged_tuple(&mut self, _: &[Self::Tree]) -> Result<Self::Tree, Self::Error> {
        unimplemented!()
    }

    fn done(mut self) -> Result<(Self::Data, Self::Statistics), Self::Error> {
        use labels:: { ExplicitIndexLabeler, MRUDeltaLabeler, MRULabeler, ParentPredictionFrequencyLabeler, RawLabeler };
        self.number_references()?;

        let mut tag_instances = HashMap::new();
        let mut string_instances = HashMap::new();
        let mut identifier_definition_instances = HashMap::new();
        let mut identifier_reference_instances = HashMap::new();
        self.root.borrow_mut().with_labels(&mut |label: &Label| {
            match *label {
                Label::String(_) => {
                    let mut entry = string_instances.entry(label.clone())
                        .or_insert(0);
                    *entry += 1;
                }
                Label::Tag(_) => {
                    let mut entry = tag_instances.entry(label.clone())
                        .or_insert(0);
                    *entry += 1;
                }
                Label::Declare(_) => {
                    let mut entry = identifier_definition_instances.entry(label.clone())
                        .or_insert(0);
                    *entry += 1;
                }
                Label::LiteralReference(_) | Label::NumberedReference(_) => {
                    let mut entry = identifier_reference_instances.entry(label.clone())
                        .or_insert(0);
                    *entry += 1;
                }
                _ => {}
            }
        });
        debug!(target: "multistream", "Detected {} identifier definitions, {} occurrences (max occurrences of each {})",
            identifier_definition_instances.len(),
            identifier_definition_instances.values()
                .cloned()
                .sum::<usize>(),
            identifier_definition_instances.values()
                .cloned()
                .max()
                .unwrap_or(0),
        );
        debug!(target: "multistream", "Detected {} identifier references, {} occurrences (max occurrences of each {})",
            identifier_reference_instances.len(),
            identifier_reference_instances.values()
                .cloned()
                .sum::<usize>(),
            identifier_reference_instances.values()
                .cloned()
                .max()
                .unwrap_or(0),
        );
        debug!(target: "multistream", "Detected {} tag definitions, {} occurrences (max occurrences of each {})",
            tag_instances.len(),
            tag_instances.values()
                .cloned()
                .sum::<usize>(),
            tag_instances.values()
                .cloned()
                .max()
                .unwrap_or(0),
        );
        debug!(target: "multistream", "Detected {} strings definitions, {} occurrences (max occurrences of each {})",
            string_instances.len(),
            string_instances.values()
                .cloned()
                .sum::<usize>(),
            string_instances.values()
                .cloned()
                .max()
                .unwrap_or(0),
        );
        debug!(target: "multistream", "Strings {:?}",
            string_instances.iter()
                .sorted_by(|a, b| usize::cmp(&b.1, &a.1))
                .into_iter()
                .format("\n"));

        let tag_frequencies : HashMap<_, _> = tag_instances.into_iter()
            .sorted_by(|a,b| usize::cmp(&b.1, &a.1))
            .into_iter()
            .enumerate()
            .map(|(position, (s, _))| (s, position))
            .collect();
        let tag_frequency_dictionary = ExplicitIndexLabeler::new(tag_frequencies.clone());

        let strings_ordered = lay_out_string_table(&mut string_instances);
        debug!(target: "multistream", "strings_ordered {:?}",
               strings_ordered.iter().format("\n"));
        let string_index_map: HashMap<Label,usize> = strings_ordered
            .iter()
            .enumerate()
            .map(|(pos, s)| (s.clone(), pos))
            .collect();
        // TODO: Use the MRU delta labeler here. The explicit index
        // labeler numbers the entries as they are output; instead it
        // can just write the string and use the implicit order.
        //let mut string_frequency_dictionary = ExplicitIndexLabeler::new(string_index_map.clone());
        let mut string_frequency_dictionary = MRUDeltaLabeler::new(true, string_index_map.clone());
        let mut string_frequency_stream = self.targets.header_strings;

        let identifier_reference_frequencies : HashMap<_, _> = identifier_reference_instances.into_iter()
            .sorted_by(|a,b| usize::cmp(&b.1, &a.1))
            .into_iter()
            .enumerate()
            .map(|(position, (s, _))| (s, position))
            .collect();
        let identifier_reference_frequency_dictionary = ExplicitIndexLabeler::new(identifier_reference_frequencies.clone());

        let identifier_definition_frequencies : HashMap<_, _> = identifier_definition_instances.into_iter()
            .sorted_by(|a,b| usize::cmp(&b.1, &a.1))
            .into_iter()
            .enumerate()
            .map(|(position, (s, _))| (s, position))
            .collect();
        let identifier_definition_frequency_dictionary = ExplicitIndexLabeler::new(identifier_definition_frequencies.clone());

        let mut header_tags = Compressor {
            dictionary: Box::new(ParentPredictionFrequencyLabeler::new(tag_frequency_dictionary)),
            stream: self.targets.header_tags.clone(),
        };

        let mut header_identifiers = Compressor {
            dictionary: Box::new(identifier_definition_frequency_dictionary),
            stream: self.targets.header_identifiers.clone(),
        };
/*
        let mut header_strings = Compressor {
            //dictionary: Box::new(MRULabeler::new()), // FIXME: Experiment with ParentPredictionFrequencyLabeler
            dictionary: Box::new(string_frequency_dictionary),
            stream: self.targets.header_strings,
        };
*/
        if let DictionaryPlacement::Header = self.options.dictionary_placement {
            debug!(target: "multistream", "Writing header");
            use bytes::varnum::WriteVarNum;
            // Pre-write tags and strings.
            // First, list of lengths (probably hard to compress), followed by a single concatenated string (normally, easy to compress).
            // FIXME: See if we need to rewrite the string to improve compression, e.g. Burrows–Wheeler transform
            header_tags.stream.write_varnum(tag_frequencies.len() as u32).unwrap();
            for tag in tag_frequencies.keys() {
                header_tags.stream.write_varnum(tag.string_byte_len() as u32).unwrap();
            }
            for tag in tag_frequencies.keys() {
                header_tags.dictionary.write_label(tag, None, &mut header_tags.stream).unwrap();
            }
            debug!(target: "multistream", "Wrote {} bytes ({} tags) to header",
                header_tags.stream.len(),
                tag_frequencies.len());

            header_identifiers.stream.write_varnum(identifier_definition_frequencies.len() as u32).unwrap();
            for id in identifier_definition_frequencies.keys() {
                header_identifiers.stream.write_varnum(id.string_byte_len() as u32).unwrap();
            }
            for id in identifier_definition_frequencies.keys() {
                header_identifiers.dictionary.write_label(id, None, &mut header_identifiers.stream).unwrap();
            }
            debug!(target: "multistream", "Wrote {} bytes ({} identifiers) to header",
                header_identifiers.stream.len(),
                identifier_definition_frequencies.len());

            string_frequency_stream.write_varnum(strings_ordered.len() as u32).unwrap();
            for ref string in &strings_ordered {
                string_frequency_stream.write_varnum(string.string_byte_len() as u32).unwrap();
            }
            for ref string in &strings_ordered {
                string_frequency_dictionary.write_label(string, None, &mut string_frequency_stream).unwrap();
            }
            debug!(target: "multistream", "Wrote {} bytes ({} strings) to header",
                string_frequency_stream.len(),
                strings_ordered.len());
        }

        let mut compressors = PerCategory {
            declarations: Compressor {
                dictionary: header_identifiers.dictionary, // Reuse dictionary.
                stream: self.targets.contents.declarations,
            },
            idrefs: Compressor {
                //                dictionary: Box::new(RawLabeler::new()), // FIXME: Could possibly use ParentPrediction where the `Scope` is the parent
                dictionary: Box::new(MRUDeltaLabeler::new(false, identifier_reference_frequencies)),
                stream: self.targets.contents.idrefs,
            },
            tags: Compressor {
                dictionary: header_tags.dictionary, // Reuse dictionary.
                stream: self.targets.contents.tags,
            },
            numbers: Compressor {
                dictionary: Box::new(RawLabeler::new()), // FIXME: Experiment with MRULabeler
                stream: self.targets.contents.numbers,
            },
            bools: Compressor {
                dictionary: Box::new(RawLabeler::new()),
                stream: self.targets.contents.bools,
            },
            lists: Compressor {
                dictionary: Box::new(RawLabeler::new()), // FIXME: Experiment with ParentPredictionFrequencyLabeler
                stream: self.targets.contents.lists,
            },
            strings: Compressor {
                dictionary: Box::new(string_frequency_dictionary), // Reuse dictionary.
                stream: self.targets.contents.strings,
            },
        };

        // Write the tree to the various streams.
        self.root.borrow().serialize_label(None, &mut compressors)
            .unwrap_or_else(|_| unimplemented!());
        self.root.borrow().serialize_children(&self.options, None, &mut compressors)
            .unwrap_or_else(|_| unimplemented!());

        let stats = Statistics {
            header_tags: header_tags.stream.len(),
            header_strings: string_frequency_stream.len(),
            contents: PerCategory {
                declarations: compressors.declarations.stream.len(),
                idrefs: compressors.idrefs.stream.len(),
                tags: compressors.tags.stream.len(),
                strings: compressors.strings.stream.len(),
                numbers: compressors.numbers.stream.len(),
                bools: compressors.bools.stream.len(),
                lists: compressors.lists.stream.len(),
            }
        };

        debug!(target: "multistream", "Compression complete: {:?}", stats);

        let mut result = vec![];
        let mut start = 0;
        result.extend_from_slice(header_tags.stream.done().unwrap().0.as_ref());
        debug!(target: "parts", "{:},{:},{:}", start, result.len(), "header_tags");
        start = result.len();

        result.extend_from_slice(header_identifiers.stream.done().unwrap().0.as_ref());
        debug!(target: "parts", "{:},{:},{:}", start, result.len(), "header_identifiers");
        start = result.len();

        result.extend_from_slice(string_frequency_stream.done().unwrap().0.as_ref());
        debug!(target: "parts", "{:},{:},{:}", start, result.len(), "string_frequency_stream");
        start = result.len();

        result.extend_from_slice(compressors.declarations.stream.done().unwrap().0.as_ref());
        debug!(target: "parts", "{:},{:},{:}", start, result.len(), "declarations");
        start = result.len();

        result.extend_from_slice(compressors.idrefs.stream.done().unwrap().0.as_ref());
        debug!(target: "parts", "{:},{:},{:}", start, result.len(), "idrefs");
        start = result.len();

        result.extend_from_slice(compressors.tags.stream.done().unwrap().0.as_ref());
        debug!(target: "parts", "{:},{:},{:}", start, result.len(), "tags");
        start = result.len();

        result.extend_from_slice(compressors.strings.stream.done().unwrap().0.as_ref());
        debug!(target: "parts", "{:},{:},{:}", start, result.len(), "strings");
        start = result.len();

        result.extend_from_slice(compressors.numbers.stream.done().unwrap().0.as_ref());
        debug!(target: "parts", "{:},{:},{:}", start, result.len(), "numbers");
        start = result.len();

        result.extend_from_slice(compressors.bools.stream.done().unwrap().0.as_ref());
        debug!(target: "parts", "{:},{:},{:}", start, result.len(), "bools");
        start = result.len();

        result.extend_from_slice(compressors.lists.stream.done().unwrap().0.as_ref());
        debug!(target: "parts", "{:},{:},{:}", start, result.len(), "lists");
        start = result.len();

        Ok((result, stats))
    }
}



/*
    fn children(&self) -> impl Iterator<Item = &SharedTree> {
        self.children.iter()
    }
    fn iter(&self) -> impl Iterator<Item = &SharedTree> {
        self.children.iter()
            .cloned()
            .flat_map(|child| {
                let borrow = child.borrow();
                borrow.iter()
            })
    }
*/

/*
/*
impl SubTree {
    fn iter(&self) -> impl Iterator<Item = &Label> {

    }
}
*/
enum Walk {
    Pop,
    Push(SharedTree)
}
struct SubTreePreIterator<'a> {
    phantom: std::marker::PhantomData<&'a ()>,
    stack: Vec<(SharedTree, usize)>,
}
impl<'a> Iterator for SubTreePreIterator<'a> {
    type Item = &'a Label;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let walk = match self.stack.last_mut() {
                None => return None,
                Some((ref node, ref mut position)) if *position == 0 => {
                    // Start visiting node.
                    *position = 1;
                    return Some(&node.borrow().label)
                },
                Some((ref node, ref position)) if *position >= node.borrow().children.len() => {
                    // We have finished visiting the node, get back up.
                    Walk::Pop
                }
                Some((ref node, ref mut position)) => {
                    // We have finished visiting a child, continue to next child.
                    let index = *position;
                    *position += 1;
                    Walk::Push(node.borrow().children[index].clone())
                }
            };
            match walk {
                Walk::Pop => {
                    self.stack.pop();
                }
                Walk::Push(subtree) => {
                    self.stack.push((subtree, 0));
                }
            }
        }
    }
}

*/
