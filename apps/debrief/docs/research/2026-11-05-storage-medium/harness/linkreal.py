import csv,os
S='/Volumes/d7sst7/dev-targets/scratch/debrief-folder-repro'; O='/Volumes/d7sst7/dev-targets/scratch/storage-research/data/src-real'
n=b=0
for r in csv.DictReader(open(S+'/manifest.csv')):
    if r['kind'] not in ('plain','stamped','partly') or int(r['bytes'])>(48<<20): continue
    p=os.path.join(O,r['path']); os.makedirs(os.path.dirname(p),exist_ok=True); os.link(os.path.join(S,'bundle',r['path']),p); n+=1; b+=int(r['bytes'])
print('linked',n,b)
