#include "ferrule_host.h"

/* Export only the probe. A client must reject this before resolving v1 calls. */
uint32_t ferrule_host_abi_version(void) {
    return FERRULE_HOST_ABI_VERSION + 1;
}
