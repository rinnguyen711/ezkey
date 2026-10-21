/* C interface to the ezkey Telex engine (core/src/ffi.rs). */
#ifndef EZKEY_H
#define EZKEY_H

#include <stdbool.h>
#include <stdint.h>

typedef struct EzkeyEngine EzkeyEngine;

EzkeyEngine *ezkey_engine_new(void);
void ezkey_engine_free(EzkeyEngine *engine);

/* Returns -1 to let the key through. Otherwise swallow the key, send
 * *backspaces backspaces, then type the returned number of code points
 * from out. */
int32_t ezkey_engine_process(EzkeyEngine *engine, uint32_t key,
                             uint32_t *backspaces, uint32_t *out,
                             uint32_t out_cap);
void ezkey_engine_backspace(EzkeyEngine *engine);
void ezkey_engine_reset(EzkeyEngine *engine);
void ezkey_engine_set_modern_style(EzkeyEngine *engine, bool modern);
/* false = Telex, true = VNI */
void ezkey_engine_set_vni(EzkeyEngine *engine, bool vni);

#endif
