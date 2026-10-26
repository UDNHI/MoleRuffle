// Native macOS helper: apply a hash-checked bank patch without Python or Java.
#include <CommonCrypto/CommonDigest.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <zlib.h>
#define LIMIT (64u * 1024u * 1024u)
static void fail(const char *s) { fprintf(stderr,"Bank patch: %s\n",s); exit(1); }
static uint32_t u32(const unsigned char *p) { return p[0]|((uint32_t)p[1]<<8)|((uint32_t)p[2]<<16)|((uint32_t)p[3]<<24); }
static void put32(unsigned char *p,uint32_t x) { for(int i=0;i<4;i++)p[i]=(unsigned char)(x>>(i*8)); }
static unsigned char *readfile(const char *p,size_t *n) {
    FILE *f=fopen(p,"rb"); if(!f)fail("cannot open input");
    if(fseek(f,0,SEEK_END))fail("seek failed"); long length=ftell(f);
    if(length<0 || length>LIMIT)fail("invalid input size"); *n=(size_t)length;
    rewind(f); unsigned char *b=malloc(*n+1); if(!b)fail("out of memory");
    if(fread(b,1,*n,f)!=*n)fail("read failed"); fclose(f); return b;
}
int main(int argc,char **argv) {
    if(argc!=4)fail("usage: bank-patch original.swf bank.patch output.swf");
    size_t sn,pn; unsigned char *s=readfile(argv[1],&sn),*p=readfile(argv[2],&pn);
    if(sn<8 || pn<80 || memcmp(p,"MBP1",4) || memcmp(s,"CWS",3))fail("invalid input format");
    unsigned char digest[CC_SHA256_DIGEST_LENGTH]; CC_SHA256(s,(CC_LONG)sn,digest);
    if(memcmp(digest,p+4,32))fail("server asset version changed; contact the test maintainer");
    uint32_t oldsize=u32(p+68),newsize=u32(p+72),count=u32(p+76);
    if(oldsize>LIMIT || newsize>LIMIT || count>1000 || u32(s+4)!=oldsize+8)fail("invalid patch size");
    unsigned char *old=malloc(oldsize),*out=malloc(newsize); if(!old||!out)fail("out of memory");
    uLongf size=oldsize; if(uncompress(old,&size,s+8,sn-8)!=Z_OK || size!=oldsize)fail("decompression failed");
    size_t pc=80,oc=0,nc=0;
    for(uint32_t i=0;i<count;i++) {
        if(pc>pn || pn-pc<12)fail("truncated patch");
        uint32_t pos=u32(p+pc),remove=u32(p+pc+4),add=u32(p+pc+8);pc+=12;
        if(pos<oc || pos>oldsize || remove>oldsize-pos || add>pn-pc)fail("invalid edit");
        size_t keep=pos-oc;
        if(nc>newsize || keep>newsize-nc || add>newsize-nc-keep)fail("output overflow");
        memcpy(out+nc,old+oc,keep);nc+=keep;
        memcpy(out+nc,p+pc,add);nc+=add;pc+=add;oc=pos+remove;
    }
    if(pc!=pn || oc>oldsize || nc>newsize || oldsize-oc!=newsize-nc)fail("size mismatch");
    memcpy(out+nc,old+oc,oldsize-oc); CC_SHA256(out,newsize,digest);
    if(memcmp(digest,p+36,32))fail("output integrity check failed");
    uLongf cap=compressBound(newsize); unsigned char *result=malloc(cap+8);if(!result)fail("out of memory");
    memcpy(result,s,8);put32(result+4,newsize+8);
    if(compress2(result+8,&cap,out,newsize,Z_DEFAULT_COMPRESSION)!=Z_OK)fail("compression failed");
    FILE *f=fopen(argv[3],"wb");if(!f)fail("cannot write output");
    if(fwrite(result,1,cap+8,f)!=cap+8 || fclose(f))fail("write failed");
    free(s);free(p);free(old);free(out);free(result); puts("Bank compatibility patch ready.");return 0;
}
