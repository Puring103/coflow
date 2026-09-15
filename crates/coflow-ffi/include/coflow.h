#ifndef COFLOW_H
#define COFLOW_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

/* UTF-8 输入只在调用期间借用；返回句柄必须通过 coflow_release 释放。 */
typedef struct CoflowResponse {
    uint64_t handle;
    int64_t integer;
    double number;
    uint64_t length;
    uint32_t tag;
    uint32_t error;
} CoflowResponse;

typedef enum CoflowOperation {
    COFLOW_LOAD_CONTRACT = 1,
    COFLOW_NEW_COMPILER = 3,
    COFLOW_ADD_CFT = 4,
    COFLOW_ADD_DIMENSION = 5,
    COFLOW_COMPILE_CONTRACT = 6,
    COFLOW_CONTRACT_BYTES = 7,
    COFLOW_CONTRACT_IDENTITY = 8,
    COFLOW_RUNTIME_CONTRACT_IDENTITY = 9,
    COFLOW_NEW_BUILDER = 10,
    COFLOW_ADD_CFD = 11,
    COFLOW_BUILD_RUNTIME = 12,
    COFLOW_RECORD = 20,
    COFLOW_RECORD_COUNT = 21,
    COFLOW_RECORD_AT = 22,
    COFLOW_FIELD = 23,
    COFLOW_DESCRIBE = 24,
    COFLOW_TEXT = 25,
    COFLOW_ARRAY_AT = 26,
    COFLOW_DICT_KEY_AT = 27,
    COFLOW_DICT_VALUE_AT = 28,
    COFLOW_CALL = 29,
    COFLOW_TYPE_NAME = 30,
    COFLOW_PROGRAM_SOURCE = 31,
    COFLOW_RETAIN_VALUE = 33,
    COFLOW_DIMENSION_VALUE = 34,
    COFLOW_VALUE_EQUALS = 35,
    COFLOW_DIMENSION_DEFAULT = 36,
    COFLOW_BUFFER_LENGTH = 40,
    COFLOW_NEW_BUFFER = 41
} CoflowOperation;

/* 返回 0 表示成功；失败信息通过 out.handle 的 UTF-8 缓冲区返回。 */
uint32_t coflow_request(uint32_t operation, uint64_t handle,
    const uint8_t *key, size_t key_length, const uint8_t *data,
    size_t data_length, uint64_t index, CoflowResponse *out);
uint32_t coflow_buffer_copy(uint64_t handle, uint8_t *destination, size_t capacity);
void coflow_release(uint64_t handle);

/* 回调同步执行，异常须在宿主内捕获并转换为 error 和消息缓冲区。
 * operation=0 查询成员类型文本，operation=1 读取成员数据。
 * 返回的缓冲区句柄所有权转移给 Rust；release 可能发生在终结线程。
 */
typedef void (*CoflowHostCallback)(uint64_t context, uint32_t operation,
    const uint8_t *field, size_t field_length, CoflowResponse *out);
typedef void (*CoflowHostRelease)(uint64_t context);
uint32_t coflow_bind_host(uint64_t builder, const uint8_t *service,
    size_t service_length, uint64_t context, CoflowHostCallback callback,
    CoflowHostRelease release);

#ifdef __cplusplus
}
#endif
#endif
