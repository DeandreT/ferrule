#include "ferrule_host.h"

#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void assert_contains(const FerruleHostBuffer *buffer, const char *text) {
    size_t length;
    const uint8_t *bytes = ferrule_host_v1_buffer_data(buffer, &length);
    assert(bytes != NULL && length > 0);
    char *copy = malloc(length + 1);
    assert(copy != NULL);
    memcpy(copy, bytes, length);
    copy[length] = '\0';
    assert(strstr(copy, text) != NULL);
    free(copy);
}

int main(int argc, char **argv) {
    assert(argc == 2);
    assert(ferrule_host_abi_version() == FERRULE_HOST_ABI_VERSION);
    FILE *file = fopen(argv[1], "rb");
    assert(file != NULL && fseek(file, 0, SEEK_END) == 0);
    long end = ftell(file);
    assert(end > 0 && fseek(file, 0, SEEK_SET) == 0);
    size_t length = (size_t)end;
    uint8_t *input = malloc(length);
    assert(input != NULL);
    assert(fread(input, 1, length, file) == length);
    assert(fclose(file) == 0);

    FerruleHostProject *project = NULL;
    FerruleHostBuffer *error = NULL;
    assert(ferrule_host_v1_load(input, length, (uint32_t)length, &project, &error)
           == FERRULE_HOST_OK);
    assert(project != NULL && error == NULL);
    memset(input, 0, length);
    free(input);

    FerruleHostBuffer *inspection = NULL;
    FerruleHostBuffer *validation = NULL;
    FerruleHostBuffer *failure = NULL;
    assert(ferrule_host_v1_inspect(project, 8192, 2048, &inspection) == FERRULE_HOST_OK);
    assert(ferrule_host_v1_validate(project, 8192, 2048, 32768, 1, &validation)
           == FERRULE_HOST_OK);
    assert(ferrule_host_v1_inspect(project, 1, 2048, &failure)
           == FERRULE_HOST_INSPECTION_LIMIT_EXCEEDED);
    assert(inspection != validation && inspection != failure && validation != failure);

    size_t inspection_length;
    const uint8_t *inspection_bytes = ferrule_host_v1_buffer_data(inspection, &inspection_length);
    assert(inspection_bytes != NULL && inspection_length > 0);
    ferrule_host_v1_project_free(project);
    assert_contains(validation, "\"functionId\":\"9007199254741027\"");
    assert_contains(validation, "\"truncated\":true");
    ferrule_host_v1_buffer_free(validation);
    assert_contains(failure, "\"code\":\"inspectionItemLimit\"");
    ferrule_host_v1_buffer_free(failure);

    size_t same_length;
    assert(ferrule_host_v1_buffer_data(inspection, &same_length) == inspection_bytes);
    assert(same_length == inspection_length);
    assert_contains(inspection, "\"pointer\":\"/document/source\"");
    ferrule_host_v1_buffer_free(inspection);

    project = NULL;
    assert(ferrule_host_v1_load((const uint8_t *)"{", 1, 1, &project, &error)
           == FERRULE_HOST_DECODE_FAILED);
    assert(project == NULL);
    assert_contains(error, "\"code\":\"decodeFailed\"");
    ferrule_host_v1_buffer_free(error);

    error = NULL;
    assert(ferrule_host_v1_load(NULL, 1, 1, &project, &error) == FERRULE_HOST_INVALID_ARGUMENT);
    assert(project == NULL);
    assert_contains(error, "\"code\":\"invalidArgument\"");
    ferrule_host_v1_buffer_free(error);

    size_t zero = 17;
    assert(ferrule_host_v1_buffer_data(NULL, &zero) == NULL && zero == 0);
    ferrule_host_v1_project_free(NULL);
    ferrule_host_v1_buffer_free(NULL);
    puts("C ABI ownership and error tests passed.");
}
