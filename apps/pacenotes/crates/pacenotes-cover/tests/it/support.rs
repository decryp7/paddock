use pacenotes_core::{CommitHash, RepoPath};
use pacenotes_cover::{CoverageReport, Format, ImportContext, PathMapper, import};

pub fn rev(c: char) -> CommitHash {
    CommitHash::parse(&c.to_string().repeat(40)).unwrap()
}

pub fn p(s: &str) -> RepoPath {
    RepoPath::parse(s).unwrap()
}

pub fn read(text: &str, format: Format, mapper: &PathMapper) -> CoverageReport {
    import(text, format, &ImportContext::new(mapper, Some(rev('a')))).unwrap()
}

pub fn hits(r: &CoverageReport, file: &str) -> Vec<(u32, u64)> {
    r.files[&p(file)].lines.iter().map(|(l, h)| (*l, *h)).collect()
}

pub const LCOV: &str = "TN:unit\nSF:/ci/work/repo/src/a.c\nFN:3,main\nFNDA:2,main\nDA:3,2\nDA:4,0\nDA:5,7,abc123\nBRDA:4,0,0,1\nBRDA:4,0,1,-\nBRDA:4,1,0,0\nend_of_record\nSF:/ci/work/repo/src/b.c\nDA:1,1\nend_of_record\nSF:/usr/include/stdio.h\nDA:9,9\nend_of_record\n";

pub const COBERTURA: &str = r#"<?xml version="1.0"?>
<coverage line-rate="0.5" branch-rate="0.5" version="1.9" timestamp="1">
  <sources><source>/ci/work/repo/app</source></sources>
  <packages><package name="app"><classes>
    <class name="A" filename="src/A.py" line-rate="0.5"><lines>
      <line number="1" hits="1" branch="false"/>
      <line number="2" hits="0" branch="true" condition-coverage="50% (1/2)"/>
      <line number="0" hits="1"/>
    </lines></class>
    <class name="B" filename="/elsewhere/B.py"><lines><line number="1" hits="1"/></lines></class>
  </classes></package></packages>
</coverage>"#;

pub const JACOCO: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<!DOCTYPE report PUBLIC "-//JACOCO//DTD Report 1.1//EN" "report.dtd">
<report name="demo">
  <package name="com/acme"><sourcefile name="A.java">
    <line nr="3" mi="0" ci="4" mb="0" cb="0"/>
    <line nr="4" mi="2" ci="0" mb="2" cb="0"/>
    <line nr="5" mi="1" ci="3" mb="1" cb="1"/>
  </sourcefile></package>
</report>"#;

pub const LLVM: &str = r#"{"data":[{"files":[{"filename":"/ci/work/repo/src/m.rs","segments":[
 [1,10,5,true,true,false],
 [3,2,0,true,true,false],
 [5,1,0,false,false,false],
 [7,1,2,true,true,false],
 [7,20,9,true,true,false],
 [8,1,0,false,false,false]
],"summary":{}}],"totals":{}}],"type":"llvm.coverage.json.export","version":"2.0.1"}"#;

pub const ISTANBUL: &str = r#"{"/ci/work/repo/src/x.js":{"path":"/ci/work/repo/src/x.js",
 "statementMap":{"0":{"start":{"line":1,"column":0},"end":{"line":1,"column":9}},"1":{"start":{"line":2,"column":0},"end":{"line":2,"column":9}},"2":{"start":{"line":2,"column":10},"end":{"line":2,"column":19}}},
 "s":{"0":3,"1":0,"2":4},
 "branchMap":{"0":{"line":2,"type":"if","locations":[{},{}]}},
 "b":{"0":[4,0]},
 "fnMap":{},"f":{}}}"#;

pub fn ci_mapper() -> PathMapper {
    PathMapper::new().strip_prefix("/ci/work/repo")
}
