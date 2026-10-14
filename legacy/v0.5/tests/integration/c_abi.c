typedef unsigned long U;
struct Pair {U a,b;};
struct Triple {U a,b,c;};
extern U back_pair(struct Pair);
struct Triple split_pair(U a,U b,U c,U d,U e,U f,struct Pair p) {
 return (struct Triple){p.a,p.b,back_pair(p)+a+b+c+d+e+f};
}

struct Small { unsigned int value; };
extern struct Small back_small(struct Small);
struct Small small_roundtrip(struct Small value) { return back_small(value); }
unsigned int ten_scalars(unsigned int a,unsigned int b,unsigned int c,unsigned int d,unsigned int e,unsigned int f,unsigned int g,unsigned int h,unsigned int i,unsigned int j) { return a+b+c+d+e+f+g+h+i+j; }

extern struct Pair back_pair_return(struct Pair);
extern struct Triple back_triple(struct Triple);
struct Pair pair_roundtrip(struct Pair value) { return back_pair_return(value); }
struct Triple triple_roundtrip(struct Triple value) { return back_triple(value); }
