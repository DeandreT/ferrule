#ifndef FERRULE_HOST_H
#define FERRULE_HOST_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define FERRULE_HOST_ABI_VERSION 1u

typedef struct FerruleHostProject FerruleHostProject;
typedef struct FerruleHostBuffer FerruleHostBuffer;
typedef uint32_t FerruleHostStatus;

enum {
    FERRULE_HOST_OK = 0,
    FERRULE_HOST_INVALID_ARGUMENT = 1,
    FERRULE_HOST_DECODE_FAILED = 2,
    FERRULE_HOST_UNSUPPORTED_PROJECT_VERSION = 3,
    FERRULE_HOST_PROJECT_TOO_LARGE = 4,
    FERRULE_HOST_INSPECTION_LIMIT_EXCEEDED = 5,
    FERRULE_HOST_VALIDATION_LIMIT_EXCEEDED = 6,
    FERRULE_HOST_INTERNAL_ERROR = 7
};

/* Query before using any versioned symbol. */
uint32_t ferrule_host_abi_version(void);

/*
 * Borrow bytes for this call only. bytes may be NULL when length is zero.
 * On success, *project owns a handle and *error is NULL. On failure,
 * *project is NULL and *error may own a UTF-8 JSON diagnostic.
 */
FerruleHostStatus ferrule_host_v1_load(
    const uint8_t *bytes, size_t length, uint32_t maximum_bytes,
    FerruleHostProject **project, FerruleHostBuffer **error);

/*
 * Each call returns its own allocated UTF-8 JSON buffer through result, for
 * either the report or structured failure. Status determines which it holds.
 * Calls may use the same project concurrently. Keep it live until all calls
 * complete. A report buffer lives independently of its project.
 */
FerruleHostStatus ferrule_host_v1_inspect(
    const FerruleHostProject *project, uint32_t maximum_items,
    uint32_t maximum_text_bytes, FerruleHostBuffer **result);
FerruleHostStatus ferrule_host_v1_validate(
    const FerruleHostProject *project, uint32_t maximum_items,
    uint32_t maximum_text_bytes, uint32_t maximum_issues,
    uint32_t maximum_diagnostics, FerruleHostBuffer **result);

/*
 * Borrow the buffer bytes until buffer_free. Bytes have no NUL terminator.
 * A NULL buffer returns NULL and sets length to zero if supplied.
 * A NULL length returns NULL.
 */
const uint8_t *ferrule_host_v1_buffer_data(
    const FerruleHostBuffer *buffer, size_t *length);

/*
 * Release each returned handle exactly once. NULL is a no-op. Complete all
 * calls and end every byte borrow before releasing the corresponding handle.
 * Every non-NULL pointer passed to the library must be valid for the documented
 * access. Output slots must be writable and must not overlap each other,
 * input bytes, or handles. Calls clear supplied output slots before work.
 */
void ferrule_host_v1_buffer_free(FerruleHostBuffer *buffer);
void ferrule_host_v1_project_free(FerruleHostProject *project);

#ifdef __cplusplus
}
#endif
#endif
