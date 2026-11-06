#!/usr/bin/env python3
"""Install only the memory/behavior harness into a clean pre-optimization checkout."""
import argparse
from pathlib import Path
import re
import shutil
import subprocess


def install_storage(root, target):
    """Add diagnostic visitors behind `fuzz`, without changing old storage or algorithms."""
    def append(relative, value):
        path = target / relative
        path.write_text(path.read_text() + "\n" + value)

    document = "crates/markview-document/"
    append(document + "Cargo.toml", '[features]\nfuzz = []\n')
    append(document + "src/lib.rs", '#[cfg(feature="fuzz")]\npub mod memory;\n')
    source = (root / (document + "src/memory.rs")).read_text()
    source = re.sub(r'visit\(\s*std::ptr::from_ref\(&\*\*i\).*?\);', '', source, flags=re.S)
    source = re.sub(r'array\(\s*&self.index.path_data,\s*"semantic_paths",\s*visit,?\s*\);', 'for path in self.index.paths.values() {array(path,"semantic_paths",visit);}', source)
    (target / (document + "src/memory.rs")).write_text(source)
    source_map = (root / (document + "src/source.rs")).read_text()
    append(document + "src/source.rs", source_map[source_map.rindex('#[cfg(feature = "fuzz")]'):])
    layout = "crates/markview-layout/"
    path = target / (layout + "Cargo.toml")
    path.write_text(path.read_text().replace('fuzz = []', 'fuzz = ["markview-document/fuzz"]'))
    path = target / (layout + "src/scene.rs")
    path.write_text(path.read_text().replace('mod memory;', 'pub(crate) mod memory;\n#[cfg(feature="fuzz")]\npub use memory::StorageVisitor;'))
    memory = (root / (layout + "src/scene/memory.rs")).read_text()
    memory = memory[memory.index('/// Known payload allocations'):]
    for name, category in [('draws', 'draw_commands'), ('text', 'text_nodes'), ('page_constraints', 'page_constraints'), ('inline_decorations', 'decorations'), ('overflow', 'overflow'), ('links', 'link_geometry')]:
        memory = re.sub(rf'self\.{name}\s*\.visit_storage\((?:"{category}",\s*)?visit\);', f'allocation(&self.{name},self.{name}.capacity(),"{category}",visit);', memory)
    append(layout + "src/scene/memory.rs", memory)
    flow = (root / (layout + "src/scene/flow.rs")).read_text()
    start = flow.index('#[cfg(feature = "fuzz")]\nimpl Scene {')
    end = flow.index('#[cfg(feature = "fuzz")]\nimpl LeafGeometry', start)
    scene = flow[start:end]
    scene = re.sub(r'visit\(\s*Arc::as_ptr\(&self\.0\).*?\);', '', scene, flags=re.S)
    flow_index = flow[flow.index('#[cfg(feature = "fuzz")]\nimpl FlowIndex {'):]
    append(layout + "src/scene/flow.rs", scene + '\n' + flow_index)
    engine = (root / (layout + "src/layout.rs")).read_text()
    engine = engine[engine.rindex('#[cfg(feature = "fuzz")]'):].replace('entry.geometry.visit_storage', 'entry.layout.visit_storage')
    append(layout + "src/layout.rs", engine)
    append(layout + "src/text.rs", '''
#[cfg(feature = "fuzz")]
impl TextNode {
 pub fn visit_storage(&self,visit:&mut crate::scene::StorageVisitor<'_>) {
  use crate::scene::memory::allocation;
  allocation(self.text.as_bytes(),self.text.capacity(),"reading_text",visit);
  allocation(&self.clusters,self.clusters.capacity(),"clusters",visit);
  allocation(&self.search_ranges,self.search_ranges.capacity(),"search_mappings",visit);
  allocation(&self.source_images,self.source_images.capacity(),"image_bindings",visit);
  allocation(&self.boundaries,self.boundaries.capacity(),"grapheme_boundaries",visit);
 }
}
''')

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("checkout", type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    target = args.checkout.resolve()
    if target == root:
        parser.error("choose an independent pre-optimization checkout")
    status = subprocess.check_output(["git", "-C", str(target), "status", "--porcelain"], text=True)
    if status:
        parser.error("baseline checkout must be clean")
    paths = [
        "fuzz/Cargo.lock",
        "fuzz/fuzzlib/Cargo.toml",
        "fuzz/fuzzlib/src/allocator.rs",
        "fuzz/fuzzlib/src/behavior.rs",
        "fuzz/fuzzlib/src/bin/doc_memory.rs",
    ]
    for relative in paths:
        shutil.copy2(root / relative, target / relative)
    lib = target / "fuzz/fuzzlib/src/lib.rs"
    lib.write_text(lib.read_text() + "\npub mod behavior;\n")
    text = target / "crates/markview-layout/src/text.rs"
    source = text.read_text()
    marker = "\tpub(crate) fn heap_bytes(&self) -> usize {"
    assert marker in source and "fn reading_metadata" not in source
    source = source.replace(marker, '''\t/// Reading metadata for representation-independent harness records.
\t#[cfg(feature = "fuzz")]
\tpub fn reading_metadata(&self) -> (&[(usize, TextCluster)], Option<usize>, Vec<usize>) {
\t\t(&self.source_images, self.selection_group, self.boundaries.clone())
\t}
''' + marker, 1)
    text.write_text(source)
    install_storage(root, target)
    print(target)


if __name__ == "__main__":
    main()
