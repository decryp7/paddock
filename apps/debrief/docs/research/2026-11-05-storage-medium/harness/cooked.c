// Throwaway: generated heap graph as plain arrays + mmap. usage: cooked gen|bfs|lookup|scan <dir> [objects] [edges]
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>
static double now(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec+t.tv_nsec*1e-9;}
static uint64_t x=88172645463325252ULL;static inline uint64_t rnd(void){x^=x<<13;x^=x>>7;x^=x<<17;return x;}
static void *mapf(const char*d,const char*n,size_t*sz){char p[600];snprintf(p,600,"%s/%s",d,n);int f=open(p,O_RDONLY);if(f<0){perror(p);exit(2);}struct stat s;fstat(f,&s);*sz=s.st_size;void*m=mmap(0,s.st_size,PROT_READ,MAP_PRIVATE,f,0);if(m==MAP_FAILED){perror("mmap");exit(2);}close(f);return m;}
int main(int c,char**v){
 const char*cmd=v[1],*d=v[2];
 if(!strcmp(cmd,"gen")){
  uint64_t O=strtoull(v[3],0,10),E=strtoull(v[4],0,10);char p[600];
  snprintf(p,600,"%s/offsets.u64",d);FILE*fo=fopen(p,"wb");snprintf(p,600,"%s/edges.u32",d);FILE*fe=fopen(p,"wb");
  snprintf(p,600,"%s/edges.csv",d);FILE*fc=c>5?fopen(p,"w"):0;
  snprintf(p,600,"%s/time.u64",d);FILE*ft=fopen(p,"wb");snprintf(p,600,"%s/stack.u32",d);FILE*fs=fopen(p,"wb");snprintf(p,600,"%s/value.u64",d);FILE*fw=fopen(p,"wb");
  uint64_t off=0;double t=now();
  for(uint64_t i=0;i<O;i++){fwrite(&off,8,1,fo);uint64_t k=(rnd()%(2*E/O+1));for(uint64_t j=0;j<k;j++){uint32_t e=(rnd()%4==0)?(uint32_t)(rnd()%O):(uint32_t)((i+1+rnd()%1000)%O);fwrite(&e,4,1,fe);}off+=k;}
  fwrite(&off,8,1,fo);fclose(fo);fclose(fe);
  for(uint64_t i=0;i<O;i++){uint64_t tt=i*1000;uint32_t st=rnd()%200000;uint64_t w=1+rnd()%100;fwrite(&tt,8,1,ft);fwrite(&st,4,1,fs);fwrite(&w,8,1,fw);}
  fclose(ft);fclose(fs);fclose(fw);
  printf("{\"step\":\"cooked-gen\",\"objects\":%llu,\"edges\":%llu,\"secs\":%.1f}\n",(unsigned long long)O,(unsigned long long)off,now()-t);return 0;}
 size_t so,se;uint64_t*off=mapf(d,"offsets.u64",&so);uint32_t*ed=mapf(d,"edges.u32",&se);uint64_t O=so/8-1,E=se/4;
 if(!strcmp(cmd,"lookup")){ // neighbours of random objects, on a cold-ish map
  double t=now();uint64_t sum=0;int L=1000000;for(int i=0;i<L;i++){uint64_t o=rnd()%O;for(uint64_t e=off[o];e<off[o+1];e++)sum+=ed[e];}
  printf("{\"step\":\"cooked-lookup\",\"lookups\":%d,\"ns_each\":%.0f,\"sum\":%llu}\n",L,(now()-t)/L*1e9,(unsigned long long)sum);return 0;}
 if(!strcmp(cmd,"validate")){ // what opening a file from elsewhere must prove: offsets monotonic, ends match, every edge in range
  double t=now();uint64_t bad=0;if(off[0]!=0||off[O]!=E)bad++;for(uint64_t i=0;i<O;i++)bad+=off[i]>off[i+1];for(uint64_t e=0;e<E;e++)bad+=ed[e]>=O;
  printf("{\"step\":\"cooked-validate\",\"bad\":%llu,\"secs\":%.3f}\n",(unsigned long long)bad,now()-t);return 0;}
 if(!strcmp(cmd,"bfs")){ // reachability from object 0 with only a bitmap and a queue on the heap
  double t=now();uint8_t*seen=calloc(O/8+1,1);uint32_t*q=malloc(O*4);uint64_t h=0,n=0;q[n++]=0;seen[0]|=1;uint64_t edges=0;
  while(h<n){uint32_t o=q[h++];for(uint64_t e=off[o];e<off[o+1];e++){uint32_t u=ed[e];edges++;if(!(seen[u/8]&(1<<(u%8)))){seen[u/8]|=1<<(u%8);q[n++]=u;}}}
  printf("{\"step\":\"cooked-bfs\",\"objects\":%llu,\"reached\":%llu,\"edges\":%llu,\"secs\":%.2f,\"heap_mb\":%.0f}\n",(unsigned long long)O,(unsigned long long)n,(unsigned long long)edges,now()-t,(O/8+O*4)/1048576.0);return 0;}
 if(!strcmp(cmd,"scan")){ // hotspots: sum value by stack in a time window, plain arrays
  size_t a,b,e2;uint64_t*tm=mapf(d,"time.u64",&a);uint32_t*st=mapf(d,"stack.u32",&b);uint64_t*w=mapf(d,"value.u64",&e2);uint64_t S=a/8;
  uint64_t*agg=calloc(200000,8);double t=now();uint64_t lo=S/4*1000,hi=S/2*1000,rows=0;for(uint64_t i=0;i<S;i++){if(tm[i]>=lo&&tm[i]<hi){agg[st[i]]+=w[i];rows++;}}
  printf("{\"step\":\"cooked-scan-hotspots\",\"samples\":%llu,\"rows\":%llu,\"secs\":%.3f}\n",(unsigned long long)S,(unsigned long long)rows,now()-t);return 0;}
 return 1;}
