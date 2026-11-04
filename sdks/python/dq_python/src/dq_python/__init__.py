from collections.abc import Iterator, Sequence
from typing import Self

import grpc

from ._generated import dq_pb2, dq_pb2_grpc
from ._generated.dq_pb2 import JobStatus as JobStatus

__all__ = ["JobStatus", "QueueClient"]


class QueueClient:
    """Synchronous client for a running DQ server."""

    def __init__(self, address: str = "127.0.0.1:50051", *, timeout: float = 5.0) -> None:
        self._channel = grpc.insecure_channel(
            address, options=[("grpc.max_receive_message_length", 8 * 1024 * 1024)]
        )
        self._queue = dq_pb2_grpc.QueueServiceStub(self._channel)
        self._timeout = timeout

    def __enter__(self) -> Self:
        return self

    def __exit__(self, *_: object) -> None:
        self.close()

    def close(self) -> None:
        self._channel.close()

    def enqueue(
        self,
        service_name: str,
        function_name: str,
        submission_key: str,
        arguments: bytes,
        *,
        caller: str = "",
    ) -> str:
        request = dq_pb2.EnqueueRequest(
            service_name=service_name,
            function_name=function_name,
            submission_key=submission_key,
            arguments=arguments,
            caller=caller,
        )
        return self._queue.Enqueue(request, timeout=self._timeout).job_id

    def get_job_status(self, job_id: str) -> dq_pb2.GetJobStatusResponse:
        return self._queue.GetJobStatus(
            dq_pb2.GetJobStatusRequest(job_id=job_id), timeout=self._timeout
        )

    def subscribe_execution(
        self,
        service_name: str,
        function_names: Sequence[str],
        *,
        timeout: float | None = None,
    ) -> Iterator[dq_pb2.ExecuteRequest]:
        request = dq_pb2.SubscribeRequest(service_name=service_name, function_names=function_names)
        return self._queue.SubscribeExecution(request, timeout=timeout)

    def subscribe_completion(
        self,
        service_name: str,
        function_names: Sequence[str],
        *,
        timeout: float | None = None,
    ) -> Iterator[dq_pb2.ExecuteResponse]:
        request = dq_pb2.SubscribeRequest(service_name=service_name, function_names=function_names)
        stream = self._queue.SubscribeCompletion(request, timeout=timeout)
        # Wait for registration so a caller can submit immediately without missing its result.
        stream.initial_metadata()
        return stream

    def complete(
        self,
        assignment: dq_pb2.ExecuteRequest,
        *,
        result: bytes | None = None,
        error: str | None = None,
    ) -> None:
        if (result is None) == (error is None):
            raise ValueError("Supply exactly one of result or error")
        request = dq_pb2.CompleteExecutionRequest(
            job_id=assignment.job_id, attempt_id=assignment.attempt_id
        )
        if result is not None:
            request.result = result
        else:
            assert error is not None
            request.error = error
        self._queue.CompleteExecution(request, timeout=self._timeout)
