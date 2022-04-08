# tango2

**tango2 is not ready for any kind of use yet. please use <https://github.com/murkland/tango> if you want to use a functional version of tango. tango2 はまだ使用できません。<https://github.com/murkland/tango> を使用してください。**

tango2 is a rewrite of <https://github.com/murkland/tango> in Rust.

## why?

https://github.com/murkland/tango/issues/50 shows that the cgo overhead of the trapping mechanism in tango1 causes a ~15% overhead, even when the hypercall does absolutely nothing: note this is also only captures the C → go direction of the call. the fastforwarder when not in a no-op state will additionally perform go → C calls to save state, which has even more overhead. the entirety of this overhead is due to cgo stack switching (the 2k go stack must be switched for a conventional C stack + some go scheduler overhead).

rust has no such overhead as there is no stack switching required, and no-op hypercalls are expected to have almost zero cost beyond the hypercall into mgba.

in tango2's current state, it is unclear how much overhead will be saved, but a minimal implementation will be coming soon!
