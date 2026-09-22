#ifndef COFLOW_H
#define COFLOW_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

/* UTF-8 输入只在调用期间借用。契约、构建器、Runtime 和缓冲区句柄需释放；
 * 值操作的 handle 返回值是 Runtime 局部值 ID（从 1 开始），不单独释放。0 表示未找到。 */
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
    COFLOW_TRY_RECORD = 32,
    COFLOW_DIMENSION_VALUE = 34,
    COFLOW_VALUE_EQUALS = 35,
    COFLOW_DIMENSION_DEFAULT = 36,
    COFLOW_SINGLETON = 37,
    COFLOW_DICT_FIND = 38,
    COFLOW_CANONICAL_VALUE = 39,
    COFLOW_BUFFER_LENGTH = 40,
    COFLOW_NEW_BUFFER = 41,
    COFLOW_RELEASE_VALUE = 42,
    COFLOW_COLLECT = 43,
    COFLOW_RETAIN_VALUE = 44,
    COFLOW_RUN_CHECKS = 45,
    COFLOW_DIMENSION_VARIANT_KEY = 46,
    COFLOW_READ_DYNAMIC_VALUE = 47,
    COFLOW_CREATE_VALUE_LEASE = 48,
    COFLOW_READ_RECORD = 49,
} CoflowOperation;

/* 全部句柄（包括 Contract 和缓冲区）只能由创建线程使用与释放。
 * 返回 0 表示成功，1 为 UTF-8 错误消息，2 为结构化构建诊断缓冲区。
 * value 为 Runtime 局部值 ID；非值操作传 0。
 * 诊断：u32 数量，然后每项三个 u32 长度+UTF-8 文本（code/source/message），
 * u8 是否有范围、u64 起始/结束 UTF-8 字节偏移；所有整数为小端。 */
uint32_t coflow_request(uint32_t operation, uint64_t handle, uint64_t value,
    const uint8_t *key, size_t key_length, const uint8_t *data,
    size_t data_length, uint64_t index, CoflowResponse *out);
uint32_t coflow_buffer_copy(uint64_t handle, uint8_t *destination, size_t capacity);
/* 无返回值释放：无效句柄、非创建线程或 Runtime busy 时不改变资源。 */
void coflow_release(uint64_t handle);
/* 显式释放只允许创建线程在 Runtime 空闲时执行；返回 1 表示线程或 busy 错误且句柄保持有效。 */
uint32_t coflow_dispose(uint64_t handle);
/* 当前线程存在活动 Runtime 时返回 1；成功时释放该线程回收域的全部本地资源。 */
uint32_t coflow_thread_shutdown(void);

/* 回调同步执行，异常须在宿主内捕获并转换为 error 和消息缓冲区。
 * operation=0 查询成员类型文本，operation=1 读取成员数据。
 * operation=2 调用函数：输入为 u32 长度+UTF-8 成员名，再接参数编码。
 * CALL(29) 的 data 使用相同参数编码：u32 数量，每项 u8 tag 后接载荷。
 * tag 0 无载荷；1 为 u8 bool；2 为 i32；3 为 f32 位；4 为 u32 长度+UTF-8；
 * 5 为字符串类型名+u32 enum 值；11 为 u64 Runtime 句柄+u64 局部值 ID。
 * 所有整数小端，参数只借用本次调用。空 data 表示无参数。
 * CALL 返回相同 tag，标量在 integer/number，文本在缓冲区，11 在 handle/length。
 * 返回的动态值通过 RELEASE_VALUE(42) 释放保活；COLLECT(43) 返回回收数量。
 * 借用的子值需独立存活时调用 RETAIN_VALUE(44)，每次增加的保活须配对释放。
 * 返回的缓冲区句柄所有权转移给 Rust；release 始终在创建线程执行。
 * tag=11 使用 handle=Runtime 句柄、length=局部值 ID，不转移其所有权。
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
