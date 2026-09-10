#include "config.h"
extern void worker(void);
void server(void) {
#if FEATURE
    worker();
#endif
}
