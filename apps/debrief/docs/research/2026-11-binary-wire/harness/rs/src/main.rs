// Throwaway harness: invented, fixed-seed Debrief-shaped payloads, encoded in each format; bytes go to out/, timings to out/rust.json.
use arrow_array::builder::*;
use arrow_array::types::Int8Type;
use arrow_array::*;
use arrow_ipc::reader::StreamReader;
use arrow_ipc::writer::{IpcWriteOptions, StreamWriter};
use arrow_schema::{DataType, Field as AF, Schema};
use flatbuffers::FlatBufferBuilder as Fbb;
use flatbuffers::WIPOffset;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

struct Rng(u64);
impl Rng { fn n(&mut self, m: u64) -> u64 { let mut x = self.0; x ^= x << 13; x ^= x >> 7; x ^= x << 17; self.0 = x; x % m } }
const WORDS: &[&str] = &["connection","timeout","retry","worker","request","session","cache","miss","flush","commit","rollback","handler","dispatch","queue","peer","socket","certificate","expired","gateway","upstream","latency","thread","pool","exhausted","index","segment","compaction","finished","started","failed"];
const NONASCII: &[&str] = &["接続に失敗しました","Zeitüberschreitung","сбой соединения","délai dépassé"];
const LEVELS: &[&str] = &["Verbose","Debug","Information","Warning","Error","Fatal"];
fn sentence(r: &mut Rng, min: u64, span: u64) -> String { let mut s = String::new(); let want = min + r.n(span); while (s.len() as u64) < want { if !s.is_empty() { s.push(' ');} if r.n(100)==0 { s.push_str(NONASCII[r.n(4) as usize]); } else { s.push_str(WORDS[r.n(30) as usize]); } if r.n(6)==0 { s.push_str(&format!(" {}", r.n(100000))); } } s }

#[derive(Serialize, Deserialize, Clone)] #[serde(rename_all="camelCase")]
struct Row { id:u32, ts:Option<i64>, ts_state:String, level:String, message:String, template:Option<String>, exception:Option<String>, fields:BTreeMap<String,Value>, source:u32, source_name:String, line:u32, byte_offset:u64, group:String, markers:Vec<String> }
#[derive(Serialize, Deserialize, Clone)] #[serde(rename_all="camelCase")]
struct Page { total:u64, offset:u64, rows:Vec<Row>, anchor_index:Option<u64> }
#[derive(Serialize, Deserialize, Clone)] #[serde(rename_all="camelCase")]
struct FacetValue { value:String, count:u64 }
#[derive(Serialize, Deserialize, Clone)] #[serde(rename_all="camelCase")]
struct Facet { field:String, values:Vec<FacetValue>, distinct:u64, total:u64 }
#[derive(Serialize, Deserialize, Clone)] #[serde(rename_all="camelCase")]
struct Lanes { lane_keys:Vec<String>, buckets:u32, levels:u32, counts:Vec<u32> }
#[derive(Serialize, Deserialize, Clone)] #[serde(rename_all="camelCase")]
struct LanesBin { lane_keys:Vec<String>, buckets:u32, levels:u32, #[serde(with="serde_bytes")] counts:Vec<u8> }
#[derive(Serialize, Deserialize, Clone)] #[serde(tag="type", rename_all="camelCase")]
enum Kind { Bool, Int{min:i64,max:i64,step:i64,unit:Option<String>}, Text{max_len:u32,pattern:Option<String>}, Enum{options:Vec<Opt>} }
#[derive(Serialize, Deserialize, Clone)] struct Opt { value:String, label:String }
#[derive(Serialize, Deserialize, Clone)] #[serde(rename_all="camelCase")]
struct SettingField { key:String, label:String, help:String, kind:Kind, value:Value, default_value:Value }
#[derive(Serialize, Deserialize, Clone)] #[serde(rename_all="camelCase")]
struct Section { id:String, title:String, owner:String, fields:Vec<SettingField> }
#[derive(Serialize, Deserialize, Clone)] struct Settings { sections:Vec<Section> }
#[derive(Serialize, Deserialize, Clone)] #[serde(rename_all="camelCase")]
struct CmdError { code:String, message:String, retryable:bool, #[serde(skip_serializing_if="Option::is_none", default)] hint:Option<String>, #[serde(skip_serializing_if="Option::is_none", default)] detail:Option<String>, #[serde(skip_serializing_if="Option::is_none", default)] cause:Option<String> }
#[derive(Serialize, Deserialize, Clone, Default)] #[serde(rename_all="camelCase")]
struct Query { text:String, regex:bool, case_sensitive:bool, levels:Vec<String>, from:Option<i64>, to:Option<i64>, sources:Vec<u32>, group_by:Vec<String>, #[serde(default)] exclude:Vec<String>, fields:BTreeMap<String,Vec<String>> }
#[derive(Serialize, Deserialize, Clone, Default)] #[serde(rename_all="camelCase")]
struct PageRequest { query:Query, offset:u64, limit:u32, descending:bool, anchor:Option<u64> }

fn gen_page(rows:usize, seed:u64)->Page { let mut r=Rng(seed); let keys:Vec<String>=(0..30).map(|i|format!("{}{}",WORDS[i%30],["Id","Name","Ms","Path"][i%4])).collect();
 let srcs:Vec<String>=(0..12).map(|i|format!("/var/log/services/{}/{}-{}.log",WORDS[i],WORDS[i+5],2026)).collect();
 let v=(0..rows).map(|i|{ let mut fields=BTreeMap::new(); for _ in 0..8 { let k=&keys[r.n(30) as usize]; let val= if r.n(3)==0 { json!(r.n(1_000_000)) } else { Value::String(sentence(&mut r,6,30)) }; fields.insert(k.clone(),val);} let s=r.n(12) as usize; let lv=match r.n(100){0..=1=>5,2..=8=>4,9..=20=>3,21..=70=>2,71..=90=>1,_=>0};
  Row{ id:1_000_000+i as u32, ts:if r.n(50)==0{None}else{Some(1_793_000_000_000+i as i64*37)}, ts_state:"exact".into(), level:LEVELS[lv].into(), message:sentence(&mut r,60,140), template:if r.n(10)<6{Some(sentence(&mut r,40,40))}else{None}, exception:if r.n(20)==0{Some(sentence(&mut r,1200,600))}else{None}, fields, source:s as u32, source_name:srcs[s].clone(), line:i as u32*3+1, byte_offset:i as u64*411, group:format!("{} · {}",WORDS[r.n(30) as usize],r.n(40)), markers:if r.n(10)==0{vec!["error".into()]}else{vec![]} }}).collect();
 Page{total:12_345_678,offset:100_000,rows:v,anchor_index:None} }
fn gen_facet(n:usize)->Facet{ let mut r=Rng(99); Facet{field:"serviceName".into(),values:(0..n).map(|_|FacetValue{value:sentence(&mut r,10,40),count:r.n(1_000_000)}).collect(),distinct:n as u64*3,total:9_000_000} }
fn gen_lanes()->Lanes{ let mut r=Rng(7); let (l,b,lv)=(40,2000,6); Lanes{lane_keys:(0..l).map(|i|format!("{}-{}",WORDS[i%30],i)).collect(),buckets:b,levels:lv,counts:(0..l*b as usize*lv as usize).map(|_|if r.n(3)==0{0}else{r.n(5000) as u32}).collect()} }
fn gen_settings()->Settings{ let mut r=Rng(5); Settings{sections:(0..12).map(|s|Section{id:format!("section{s}"),title:sentence(&mut r,8,12),owner:"app".into(),fields:(0..8).map(|f|{ let (kind,val)=match f%4{0=>(Kind::Bool,json!(true)),1=>(Kind::Int{min:0,max:100,step:1,unit:Some("lines".into())},json!(5)),2=>(Kind::Text{max_len:64,pattern:None},json!(sentence(&mut r,6,8))),_=>(Kind::Enum{options:(0..4).map(|o|Opt{value:format!("v{o}"),label:sentence(&mut r,5,8)}).collect()},json!("v1"))}; SettingField{key:format!("key{f}"),label:sentence(&mut r,10,20),help:sentence(&mut r,40,60),kind,value:val.clone(),default_value:val}}).collect()}).collect()} }
fn gen_error()->CmdError{ CmdError{code:"invalid".into(),message:"The time range is not valid: the end is before the start.".into(),retryable:false,hint:Some("Choose an end after the start.".into()),detail:None,cause:None} }
fn gen_req()->PageRequest{ PageRequest{query:Query{text:"connection timeout".into(),levels:vec!["Error".into(),"Warning".into()],from:Some(1_793_000_000_000),to:Some(1_793_100_000_000),sources:vec![1,2,3],group_by:vec!["serviceName".into()],..Default::default()},offset:100_000,limit:200,descending:false,anchor:None} }

fn jsonb<T:Serialize>(t:&T)->Vec<u8>{serde_json::to_vec(t).unwrap()}
fn mp<T:Serialize>(t:&T)->Vec<u8>{rmp_serde::to_vec_named(t).unwrap()}
fn cb<T:Serialize>(t:&T)->Vec<u8>{let mut o=vec![];ciborium::into_writer(t,&mut o).unwrap();o}
fn fx<T:Serialize>(t:&T)->Vec<u8>{flexbuffers::to_vec(t).unwrap()}

// postcard page: fields as JSON text, as paddock_ipc::Json does.
#[derive(Serialize)] struct PcRow<'a>{id:u32,ts:Option<i64>,level:u8,message:&'a str,template:Option<&'a str>,exception:Option<&'a str>,fields:String,source:u32,source_name:&'a str,line:u32,byte_offset:u64,group:&'a str,markers:&'a [String]}
#[derive(Serialize)] struct PcPage<'a>{total:u64,offset:u64,rows:Vec<PcRow<'a>>,anchor_index:Option<u64>}
fn lvl(s:&str)->u8{LEVELS.iter().position(|l|*l==s).unwrap() as u8}
fn pc_page(p:&Page)->Vec<u8>{ let rows=p.rows.iter().map(|r|PcRow{id:r.id,ts:r.ts,level:lvl(&r.level),message:&r.message,template:r.template.as_deref(),exception:r.exception.as_deref(),fields:serde_json::to_string(&r.fields).unwrap(),source:r.source,source_name:&r.source_name,line:r.line,byte_offset:r.byte_offset,group:&r.group,markers:&r.markers}).collect(); postcard::to_stdvec(&PcPage{total:p.total,offset:p.offset,rows,anchor_index:p.anchor_index}).unwrap() }

// Arrow.
fn ipc(b:&RecordBatch)->Vec<u8>{ let mut o=vec![]; { let mut w=StreamWriter::try_new_with_options(&mut o,b.schema_ref(),IpcWriteOptions::default()).unwrap(); w.write(b).unwrap(); w.finish().unwrap(); } o }
fn dict(vals:impl Iterator<Item=Option<String>>)->ArrayRef{ let mut b=StringDictionaryBuilder::<Int8Type>::new(); for v in vals { match v {Some(s)=>{b.append_value(s);},None=>b.append_null()} } Arc::new(b.finish()) }
fn s<'a>(vals:impl Iterator<Item=Option<&'a str>>)->ArrayRef{ let mut b=StringBuilder::new(); for v in vals { b.append_option(v);} Arc::new(b.finish()) }
type ArrayRef=arrow_array::ArrayRef;
fn arrow_page(p:&Page,use_dict:bool,dyn_fields:bool)->RecordBatch{ let r=&p.rows; let sd=|d:bool|if d{DataType::Dictionary(Box::new(DataType::Int8),Box::new(DataType::Utf8))}else{DataType::Utf8};
 let mut f=vec![AF::new("id",DataType::UInt32,false),AF::new("ts",DataType::Int64,true),AF::new("level",sd(use_dict),false),AF::new("message",DataType::Utf8,false),AF::new("template",DataType::Utf8,true),AF::new("exception",DataType::Utf8,true),AF::new("source",DataType::UInt32,false),AF::new("sourceName",sd(use_dict),false),AF::new("line",DataType::UInt32,false),AF::new("byteOffset",DataType::UInt64,false),AF::new("group",DataType::Utf8,false)];
 let mut c:Vec<ArrayRef>=vec![Arc::new(UInt32Array::from_iter_values(r.iter().map(|x|x.id))),Arc::new(Int64Array::from_iter(r.iter().map(|x|x.ts))),
  if use_dict{dict(r.iter().map(|x|Some(x.level.clone())))}else{s(r.iter().map(|x|Some(x.level.as_str())))},
  s(r.iter().map(|x|Some(x.message.as_str()))),s(r.iter().map(|x|x.template.as_deref())),s(r.iter().map(|x|x.exception.as_deref())),
  Arc::new(UInt32Array::from_iter_values(r.iter().map(|x|x.source))),
  if use_dict{dict(r.iter().map(|x|Some(x.source_name.clone())))}else{s(r.iter().map(|x|Some(x.source_name.as_str())))},
  Arc::new(UInt32Array::from_iter_values(r.iter().map(|x|x.line))),Arc::new(UInt64Array::from_iter_values(r.iter().map(|x|x.byte_offset))),s(r.iter().map(|x|Some(x.group.as_str())))];
 // markers as a list of strings
 let mut lb=ListBuilder::new(StringBuilder::new()); for x in r { for m in &x.markers { lb.values().append_value(m);} lb.append(true);} let la=lb.finish(); f.push(AF::new("markers",la.data_type().clone(),false)); c.push(Arc::new(la));
 if dyn_fields { let mut keys:Vec<&String>=r.iter().flat_map(|x|x.fields.keys()).collect(); keys.sort(); keys.dedup(); for k in keys { f.push(AF::new(format!("f.{k}"),DataType::Utf8,true)); let vals:Vec<Option<String>>=r.iter().map(|x|x.fields.get(k).map(|v|v.to_string())).collect(); c.push(s(vals.iter().map(|v|v.as_deref()))); } }
 else { f.push(AF::new("fields",DataType::Utf8,false)); let vals:Vec<String>=r.iter().map(|x|serde_json::to_string(&x.fields).unwrap()).collect(); c.push(s(vals.iter().map(|v|Some(v.as_str())))); }
 let mut md=std::collections::HashMap::new(); md.insert("total".to_string(),p.total.to_string()); md.insert("offset".to_string(),p.offset.to_string());
 RecordBatch::try_new(Arc::new(Schema::new_with_metadata(f,md)),c).unwrap() }
fn arrow_page_map(p:&Page)->RecordBatch{ let base=arrow_page(p,true,false); let schema=base.schema(); let n=schema.fields().len()-1; let mut f:Vec<AF>=schema.fields().iter().take(n).map(|x|x.as_ref().clone()).collect(); let mut c:Vec<ArrayRef>=(0..n).map(|i|base.column(i).clone()).collect(); let mut mb=MapBuilder::new(None,StringBuilder::new(),StringBuilder::new()); for r in &p.rows { for (k,v) in &r.fields { mb.keys().append_value(k); match v { Value::String(s)=>mb.values().append_value(s), o=>mb.values().append_value(o.to_string()) } } mb.append(true).unwrap(); } let m=mb.finish(); f.push(AF::new("fields",m.data_type().clone(),false)); c.push(Arc::new(m)); RecordBatch::try_new(Arc::new(Schema::new_with_metadata(f,schema.metadata().clone())),c).unwrap() }
fn arrow_facet(fc:&Facet)->RecordBatch{ let mut md=std::collections::HashMap::new(); md.insert("field".to_string(),fc.field.clone()); md.insert("distinct".to_string(),fc.distinct.to_string()); md.insert("total".to_string(),fc.total.to_string());
 RecordBatch::try_new(Arc::new(Schema::new_with_metadata(vec![AF::new("value",DataType::Utf8,false),AF::new("count",DataType::UInt64,false)],md)),vec![s(fc.values.iter().map(|v|Some(v.value.as_str()))),Arc::new(UInt64Array::from_iter_values(fc.values.iter().map(|v|v.count)))]).unwrap() }
fn arrow_lanes(l:&Lanes)->RecordBatch{ let mut md=std::collections::HashMap::new(); md.insert("laneKeys".to_string(),serde_json::to_string(&l.lane_keys).unwrap()); md.insert("buckets".to_string(),l.buckets.to_string()); md.insert("levels".to_string(),l.levels.to_string());
 RecordBatch::try_new(Arc::new(Schema::new_with_metadata(vec![AF::new("counts",DataType::UInt32,false)],md)),vec![Arc::new(UInt32Array::from(l.counts.clone()))]).unwrap() }
fn arrow_error(e:&CmdError)->RecordBatch{ RecordBatch::try_new(Arc::new(Schema::new(vec![AF::new("code",DataType::Utf8,false),AF::new("message",DataType::Utf8,false),AF::new("retryable",DataType::Boolean,false),AF::new("hint",DataType::Utf8,true),AF::new("detail",DataType::Utf8,true),AF::new("cause",DataType::Utf8,true)])),vec![s(std::iter::once(Some(e.code.as_str()))),s(std::iter::once(Some(e.message.as_str()))),Arc::new(BooleanArray::from(vec![e.retryable])),s(std::iter::once(e.hint.as_deref())),s(std::iter::once(e.detail.as_deref())),s(std::iter::once(e.cause.as_deref()))]).unwrap() }

// FlatBuffers by hand (flatc not run: the same builder calls its generated code makes).
fn vt(i:u16)->u16{4+2*i}
fn fb_page(p:&Page)->Vec<u8>{ let mut b=Fbb::with_capacity(256*1024); let mut rows=vec![]; for r in &p.rows { let msg=b.create_string(&r.message); let t=r.template.as_ref().map(|t|b.create_string(t)); let ex=r.exception.as_ref().map(|t|b.create_string(t)); let mut fs=vec![]; for (k,v) in &r.fields { let ko=b.create_string(k); let vs=v.to_string(); let vo=b.create_string(&vs); let st=b.start_table(); b.push_slot_always(vt(0),ko); b.push_slot_always(vt(1),vo); fs.push(WIPOffset::<flatbuffers::TableFinishedWIPOffset>::new(b.end_table(st).value())); } let fv=b.create_vector(&fs); let sn=b.create_shared_string(&r.source_name); let g=b.create_string(&r.group); let ms:Vec<_>=r.markers.iter().map(|m|b.create_shared_string(m)).collect(); let mv=b.create_vector(&ms);
  let st=b.start_table(); b.push_slot::<u64>(vt(11),r.byte_offset,0); b.push_slot::<i64>(vt(1),r.ts.unwrap_or(0),0); b.push_slot_always(vt(4),msg); if let Some(t)=t{b.push_slot_always(vt(5),t);} if let Some(t)=ex{b.push_slot_always(vt(6),t);} b.push_slot_always(vt(7),fv); b.push_slot_always(vt(9),sn); b.push_slot_always(vt(12),g); b.push_slot_always(vt(13),mv); b.push_slot::<u32>(vt(0),r.id,0); b.push_slot::<u32>(vt(8),r.source,0); b.push_slot::<u32>(vt(10),r.line,0); b.push_slot::<u8>(vt(3),lvl(&r.level),0); b.push_slot::<bool>(vt(2),r.ts.is_some(),false); rows.push(WIPOffset::<flatbuffers::TableFinishedWIPOffset>::new(b.end_table(st).value())); }
 let rv=b.create_vector(&rows); let st=b.start_table(); b.push_slot::<u64>(vt(0),p.total,0); b.push_slot::<u64>(vt(1),p.offset,0); b.push_slot_always(vt(2),rv); let root=b.end_table(st); b.finish(root,None); b.finished_data().to_vec() }
// Generic Frame { rows, columns:[Column{name,data_type,data}] } with union Data{U32=1,I64=2,Utf8=3,Dict=4}
struct Fr<'a,'b>{b:&'a mut Fbb<'b>,cols:Vec<WIPOffset<flatbuffers::TableFinishedWIPOffset>>}
impl<'a,'b> Fr<'a,'b>{ fn col(&mut self,name:&str,ty:u8,data:WIPOffset<flatbuffers::TableFinishedWIPOffset>){ let n=self.b.create_string(name); let st=self.b.start_table(); self.b.push_slot_always(vt(0),n); self.b.push_slot::<u8>(vt(1),ty,0); self.b.push_slot_always(vt(2),data); self.cols.push(WIPOffset::new(self.b.end_table(st).value())); }
 fn vecu32(&mut self,name:&str,v:&[u32]){ let o=self.b.create_vector(v); let st=self.b.start_table(); self.b.push_slot_always(vt(0),o); let t=WIPOffset::new(self.b.end_table(st).value()); self.col(name,1,t); }
 fn veci64(&mut self,name:&str,v:&[i64]){ let o=self.b.create_vector(v); let st=self.b.start_table(); self.b.push_slot_always(vt(0),o); let t=WIPOffset::new(self.b.end_table(st).value()); self.col(name,2,t); }
 fn utf8(&mut self,name:&str,v:impl Iterator<Item=String>){ let (mut off,mut by)=(vec![0u32],Vec::<u8>::new()); for x in v { by.extend_from_slice(x.as_bytes()); off.push(by.len() as u32);} let bo=self.b.create_vector(&by); let oo=self.b.create_vector(&off); let st=self.b.start_table(); self.b.push_slot_always(vt(0),oo); self.b.push_slot_always(vt(1),bo); let t=WIPOffset::new(self.b.end_table(st).value()); self.col(name,3,t); }
 fn dict(&mut self,name:&str,v:impl Iterator<Item=String>){ let mut vals:Vec<String>=vec![]; let keys:Vec<u8>=v.map(|x|{ match vals.iter().position(|y|*y==x){Some(i)=>i as u8,None=>{vals.push(x);(vals.len()-1) as u8}}}).collect(); let so:Vec<_>=vals.iter().map(|x|self.b.create_string(x)).collect(); let vo=self.b.create_vector(&so); let ko=self.b.create_vector(&keys); let st=self.b.start_table(); self.b.push_slot_always(vt(0),ko); self.b.push_slot_always(vt(1),vo); let t=WIPOffset::new(self.b.end_table(st).value()); self.col(name,4,t); } }
fn fb_frame(p:&Page)->Vec<u8>{ let mut b=Fbb::with_capacity(256*1024); let r=&p.rows; let mut fr=Fr{b:&mut b,cols:vec![]};
 fr.vecu32("id",&r.iter().map(|x|x.id).collect::<Vec<_>>()); fr.veci64("ts",&r.iter().map(|x|x.ts.unwrap_or(i64::MIN)).collect::<Vec<_>>()); fr.dict("level",r.iter().map(|x|x.level.clone())); fr.utf8("message",r.iter().map(|x|x.message.clone())); fr.utf8("template",r.iter().map(|x|x.template.clone().unwrap_or_default())); fr.utf8("exception",r.iter().map(|x|x.exception.clone().unwrap_or_default())); fr.utf8("fields",r.iter().map(|x|serde_json::to_string(&x.fields).unwrap())); fr.vecu32("source",&r.iter().map(|x|x.source).collect::<Vec<_>>()); fr.dict("sourceName",r.iter().map(|x|x.source_name.clone())); fr.vecu32("line",&r.iter().map(|x|x.line).collect::<Vec<_>>()); fr.utf8("group",r.iter().map(|x|x.group.clone()));
 let cols=std::mem::take(&mut fr.cols); let cv=b.create_vector(&cols); let st=b.start_table(); b.push_slot::<u32>(vt(0),r.len() as u32,0); b.push_slot_always(vt(1),cv); let root=b.end_table(st); b.finish(root,None); b.finished_data().to_vec() }
fn fb_lanes(l:&Lanes)->Vec<u8>{ let mut b=Fbb::with_capacity(2*1024*1024); let cv=b.create_vector(&l.counts); let st=b.start_table(); b.push_slot_always(vt(0),cv); b.push_slot::<u32>(vt(1),l.buckets,0); let root=b.end_table(st); b.finish(root,None); b.finished_data().to_vec() }

fn time<T>(mut f:impl FnMut()->T)->f64{ let mut v=vec![]; let t0=Instant::now(); let mut n=0; while n<15 || (t0.elapsed().as_millis()<400 && n<400) { let t=Instant::now(); std::hint::black_box(f()); v.push(t.elapsed().as_secs_f64()*1000.0); n+=1; } v.sort_by(|a,b|a.partial_cmp(b).unwrap()); v[v.len()/2] }
struct Out{dir:String,rec:Vec<Value>}
impl Out{ fn put<T:AsRef<[u8]>>(&mut self,name:&str,mut f:impl FnMut()->T){ let ms=time(||f()); let bytes=f(); std::fs::write(format!("{}/{name}",self.dir),bytes.as_ref()).unwrap(); self.rec.push(json!({"name":name,"encodeMs":ms,"bytes":bytes.as_ref().len()})); println!("{name:32} {:>10} B  encode {ms:9.3} ms",bytes.as_ref().len()); } }

static PROGRESS:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0); static CUR:std::sync::Mutex<Vec<u8>>=std::sync::Mutex::new(Vec::new());
fn mutate_loop(name:&str,seed:&[u8],n:u64,dec:&dyn Fn(&[u8])){ let mut r=Rng(0x1234_5678); let (mut panics,mut maxus)=(0u64,0u128); std::panic::set_hook(Box::new(|_|{})); let nm=name.to_string(); std::thread::spawn(move||{ let mut last=u64::MAX; loop { std::thread::sleep(std::time::Duration::from_secs(5)); let p=PROGRESS.load(std::sync::atomic::Ordering::Relaxed); if p==last { let b=CUR.lock().unwrap().clone(); println!("fuzz {nm}: input {p} still running after 5 s, {} bytes: {:02x?}",b.len(),b); std::process::exit(3);} last=p; } }); for it in 0..n { PROGRESS.store(it,std::sync::atomic::Ordering::Relaxed); let mut b=seed.to_vec(); for _ in 0..1+r.n(8) { let i=r.n(b.len() as u64) as usize; b[i]=r.n(256) as u8; } if r.n(4)==0 { let l=r.n(b.len() as u64) as usize; b.truncate(l.max(1)); } { let mut g=CUR.lock().unwrap(); g.clear(); g.extend_from_slice(&b); } let t=Instant::now(); if std::panic::catch_unwind(std::panic::AssertUnwindSafe(||dec(&b))).is_err(){panics+=1;} maxus=maxus.max(t.elapsed().as_micros()); } println!("fuzz {name}: {n} mutated inputs, {panics} panics, slowest {maxus} us"); }

// serde feature census: what Debrief's wire types use, through each self-describing format.
#[derive(Serialize,Deserialize,PartialEq,Debug,Clone)] #[serde(untagged)] enum Un { A(u32), B{ x:String } }
#[derive(Serialize,Deserialize,PartialEq,Debug,Clone)] #[serde(tag="type",rename_all="camelCase")] enum Tg { One{ a:u32 }, Two }
#[derive(Serialize,Deserialize,PartialEq,Debug,Clone)] #[serde(tag="t",content="c")] enum Adj { P(u32), Q{ v:String } }
#[derive(Serialize,Deserialize,PartialEq,Debug,Clone)] enum Ex { U, N(u32), S{ a:u32 } }
#[derive(Serialize,Deserialize,PartialEq,Debug,Clone)] struct In { b:u32 }
#[derive(Serialize,Deserialize,PartialEq,Debug,Clone)] struct Fl { a:u32, #[serde(flatten)] rest:In }
#[derive(Serialize,Deserialize,PartialEq,Debug,Clone,PartialOrd,Ord,Eq)] enum Lv { Err, Warn }
#[derive(Serialize,Deserialize,PartialEq,Debug,Clone)] struct Sk { a:u32, #[serde(skip_serializing_if="Option::is_none",default)] o:Option<u32> }
fn chk<T:Serialize+serde::de::DeserializeOwned+PartialEq+std::fmt::Debug>(name:&str,v:T){ let j=serde_json::to_string(&v).unwrap();
 let m=rmp_serde::to_vec_named(&v).map_err(|e|e.to_string()).and_then(|b|rmp_serde::from_slice::<T>(&b).map_err(|e|e.to_string())).map(|d|d==v);
 let f=flexbuffers::to_vec(&v).map_err(|e|e.to_string()).and_then(|b|flexbuffers::from_slice::<T>(&b).map_err(|e|e.to_string())).map(|d|d==v);
 let c=(||->Result<bool,String>{let mut o=vec![];ciborium::into_writer(&v,&mut o).map_err(|e|e.to_string())?; Ok(ciborium::from_reader::<T,_>(&o[..]).map_err(|e|e.to_string())?==v)})();
 let p=postcard::to_stdvec(&v).map_err(|e|e.to_string()).and_then(|b|postcard::from_bytes::<T>(&b).map_err(|e|e.to_string())).map(|d|d==v);
 println!("serde {name:26} json {j:44} msgpack {m:?} | flex {f:?} | cbor {c:?} | postcard {p:?}"); }
fn serdecheck(){ chk("untagged",vec![Un::A(1),Un::B{x:"y".into()}]); chk("internally tagged",vec![Tg::One{a:1},Tg::Two]); chk("adjacently tagged",vec![Adj::P(1),Adj::Q{v:"z".into()}]); chk("externally tagged",vec![Ex::U,Ex::N(1),Ex::S{a:1}]); chk("flatten",Fl{a:1,rest:In{b:2}}); chk("skip_serializing_if",vec![Sk{a:1,o:None},Sk{a:1,o:Some(2)}]);
 let mut bm=BTreeMap::new(); bm.insert(Lv::Err,1u64); chk("BTreeMap<enum,_>",bm); let mut im=BTreeMap::new(); im.insert(7u32,"a".to_string()); chk("BTreeMap<u32,_>",im);
 chk("u64 above 2^53",vec![u64::MAX,9007199254740993u64]); chk("i64 negative",vec![i64::MIN,-1]); chk("f64",vec![1.5f64,1e300]); chk("Value tree",json!({"a":[1,"x",null,{"b":2.5}],"c":true})); chk("Option<Option>",vec![Some(1u32),None]); chk("empty vec, empty map",(Vec::<u32>::new(),BTreeMap::<String,u32>::new())); chk("unit",());
 let nan=rmp_serde::to_vec(&f64::NAN).map(|b|b.len()); println!("serde NaN: json {:?}, msgpack {:?}",serde_json::to_string(&f64::NAN),nan); }
fn main(){ let a:Vec<String>=std::env::args().collect(); if a.get(1).map(|s|s.as_str())==Some("serde"){ serdecheck(); return; } let dir=a.get(2).cloned().unwrap_or("out".into());
 if a.get(1).map(|s|s.as_str())==Some("fuzz"){ let req=gen_req(); let page=gen_page(50,42); let n:u64=a[3].parse().unwrap(); match a[2].as_str(){
   "json"=>mutate_loop("json args",&jsonb(&req),n,&|b|{let _=serde_json::from_slice::<PageRequest>(b);}),
   "msgpack"=>mutate_loop("msgpack args",&mp(&req),n,&|b|{let _=rmp_serde::from_slice::<PageRequest>(b);}),
   "flex"=>mutate_loop("flexbuffers args",&fx(&req),n,&|b|{let _=flexbuffers::from_slice::<PageRequest>(b);}),
   "cbor"=>mutate_loop("cbor args",&cb(&req),n,&|b|{let _=ciborium::from_reader::<PageRequest,_>(b);}),
   "arrow"=>mutate_loop("arrow stream",&ipc(&arrow_page(&page,true,false)),n,&|b|{ if let Ok(mut rd)=StreamReader::try_new(b,None){ while let Some(Ok(_))=rd.next(){} } }),
   _=>{} } return; }
 std::fs::create_dir_all(&dir).unwrap(); let mut o=Out{dir:dir.clone(),rec:vec![]};
 for (tag,n) in [("200",200usize),("5000",5000)] { let p=gen_page(n,42);
  o.put(&format!("page{tag}.json"),||jsonb(&p)); o.put(&format!("page{tag}.msgpack"),||mp(&p)); o.put(&format!("page{tag}.cbor"),||cb(&p)); o.put(&format!("page{tag}.flex"),||fx(&p)); o.put(&format!("page{tag}.postcard"),||pc_page(&p));
  o.put(&format!("page{tag}.fb"),||fb_page(&p)); o.put(&format!("page{tag}.frame"),||fb_frame(&p));
  o.put(&format!("page{tag}.arrow_dict"),||ipc(&arrow_page(&p,true,false))); o.put(&format!("page{tag}.arrow_plain"),||ipc(&arrow_page(&p,false,false))); o.put(&format!("page{tag}.arrow_dyn"),||ipc(&arrow_page(&p,true,true))); o.put(&format!("page{tag}.arrow_map"),||ipc(&arrow_page_map(&p)));
  { let b=arrow_page(&p,true,false); let empty=b.slice(0,0); o.put(&format!("page{tag}.schema_only"),||ipc(&empty)); let bm=arrow_page_map(&p); let em=bm.slice(0,0); o.put(&format!("page{tag}.map_schema_only"),||ipc(&em)); let parts=5usize; let step=b.num_rows()/parts; let mut msgs:Vec<Vec<u8>>=vec![]; for i in 0..parts { msgs.push(ipc(&b.slice(i*step,step))); } let total:usize=msgs.iter().map(|m|m.len()).sum(); println!("  progressive page{tag}: 5 self-contained messages {total} B against one {} B",ipc(&b).len()); for (i,m) in msgs.iter().enumerate(){ std::fs::write(format!("{}/page{tag}.part{i}",o.dir),m).unwrap(); } }
 // Batch construction alone vs write alone.
  let b=arrow_page(&p,true,false); let build=time(||arrow_page(&p,true,false)); let write=time(||ipc(&b)); println!("  arrow page{tag}: build {build:.3} ms, write {write:.3} ms"); o.rec.push(json!({"name":format!("page{tag}.arrow_parts"),"buildMs":build,"writeMs":write})); 
 }
 let f=gen_facet(1000); o.put("facet.json",||jsonb(&f)); o.put("facet.msgpack",||mp(&f)); o.put("facet.cbor",||cb(&f)); o.put("facet.flex",||fx(&f)); o.put("facet.arrow",||ipc(&arrow_facet(&f)));
 let l=gen_lanes(); o.put("lanes.json",||jsonb(&l)); let lb=LanesBin{lane_keys:l.lane_keys.clone(),buckets:l.buckets,levels:l.levels,counts:l.counts.iter().flat_map(|c|c.to_le_bytes()).collect()}; o.put("lanes.msgpack_arr",||mp(&l)); o.put("lanes.msgpack_bin",||mp(&lb)); o.put("lanes.flex_bin",||fx(&lb)); o.put("lanes.arrow",||ipc(&arrow_lanes(&l))); o.put("lanes.fb",||fb_lanes(&l));
 let st=gen_settings(); o.put("settings.json",||jsonb(&st)); o.put("settings.msgpack",||mp(&st)); o.put("settings.cbor",||cb(&st)); o.put("settings.flex",||fx(&st));
 // Settings as a one-row Arrow batch with a JSON text column: nested types by hand are not worth writing for a throwaway; see the ADR.
 o.put("settings.arrow_json",||{ let v=serde_json::to_string(&st).unwrap(); ipc(&RecordBatch::try_new(Arc::new(Schema::new(vec![AF::new("json",DataType::Utf8,false)])),vec![s(std::iter::once(Some(v.as_str())))]).unwrap()) });
 let e=gen_error(); o.put("error.json",||jsonb(&e)); o.put("error.msgpack",||mp(&e)); o.put("error.cbor",||cb(&e)); o.put("error.flex",||fx(&e)); o.put("error.arrow",||ipc(&arrow_error(&e)));
 let r=gen_req(); o.put("req.json",||jsonb(&r)); o.put("req.msgpack",||mp(&r)); o.put("req.flex",||fx(&r)); o.put("req.cbor",||cb(&r));
 // Arg decode in Rust.
 let (rj,rm,rf,rc)=(jsonb(&r),mp(&r),fx(&r),cb(&r));
 for (n,ms) in [("req.decode.json",time(||serde_json::from_slice::<PageRequest>(&rj).unwrap())),("req.decode.msgpack",time(||rmp_serde::from_slice::<PageRequest>(&rm).unwrap())),("req.decode.flex",time(||flexbuffers::from_slice::<PageRequest>(&rf).unwrap())),("req.decode.cbor",time(||ciborium::from_reader::<PageRequest,_>(&rc[..]).unwrap()))] { println!("{n:32} {ms:.4} ms"); o.rec.push(json!({"name":n,"decodeMs":ms})); }
 std::fs::write(format!("{dir}/rust.json"),serde_json::to_string_pretty(&o.rec).unwrap()).unwrap(); }
