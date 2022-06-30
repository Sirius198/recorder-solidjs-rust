import { Component, createSignal, Match, onMount, Show, Switch } from 'solid-js';
import './assets/css/buttons.css';
import { makeid } from './utils';

const RecordingScreen: Component = () => {

    const [isConnected, setConnected] = createSignal(false);
    const [deviceFound, setDeviceFound] = createSignal(false);
    const [getRecordingId, setRecordingId] = createSignal('');
    const [getElapsedTime, setElapsedTime] = createSignal(0);
    const [getRecordingStep, setRecordingStep] = createSignal(0);

    const baseUrl = "https://vmi873667.contaboserver.net:3000";
    const websocketUrl = "wss://vmi873667.contaboserver.net:3000/websocket";
    const captureInterval = 1000;

    let mediaRecorder: MediaRecorder;
    let socket: WebSocket | null;
    let chunks: Blob[] = [];
    let localVideo: HTMLVideoElement;
    let timerId: number;
    let recordingTimerId: number;

    onMount(() => {
        setRecordingId(makeid(30));
        getMedia();
        connectServer();
    });

    const getMedia = async () => {
        const mimeType = 'video/webm;codecs=vp8,opus';

        var constraints = {
            video: true,
            audio: {
                echoCancellation: true,
            }
        };

        if (!MediaRecorder.isTypeSupported(mimeType)) {
            alert('vp8/opus mime type is not supported');
            return;
        }

        const options = {
            audioBitsPerSecond: 128000,
            mimeType,
            videoBitsPerSecond: 2500000
        };

        const mediaStream = await navigator.mediaDevices.getUserMedia(constraints);
        localVideo = document.getElementById('localVideo') as HTMLVideoElement;
        localVideo.srcObject = mediaStream;
        mediaRecorder = new MediaRecorder(mediaStream, options);
        mediaRecorder.ondataavailable = handleOnDataAvailable;

        setDeviceFound(true);
    };

    const connectServer = () => {
        socket = new WebSocket(websocketUrl);
        socket.onopen = function () {
            socket?.send(getRecordingId());
            setConnected(true);

            if (timerId != -1) {
                clearInterval(timerId);
                timerId = -1;
            }
        };
        socket.onerror = () => {
            console.log('connection error');
            setConnected(false);
            socket = null;
        };
        socket.onclose = () => {
            console.log('connection close');
            setConnected(false);
            socket = null;
            timerId = setTimeout(function () { connectServer(); }, 5000);
        };
        socket.onmessage = socketMessageHandler;
    };

    const handleOnDataAvailable = ({ data }: any) => {
        if (data.size > 0) {
            chunks.push(data);
        }
    };

    const socketMessageHandler = ({ data }: any) => {

        if (data == getRecordingId() + '.webm') {
            playSavedVideo();
            console.log('yes');
        }
    };

    const formatElapsedTime = (): string => {
        var sec_num = getElapsedTime();
        var hours = Math.floor(sec_num / 3600);
        var minutes = Math.floor((sec_num - (hours * 3600)) / 60);
        var seconds = sec_num - (hours * 3600) - (minutes * 60);

        if (hours < 10) { hours = "0" + hours; }
        if (minutes < 10) { minutes = "0" + minutes; }
        if (seconds < 10) { seconds = "0" + seconds; }
        return hours + ':' + minutes + ':' + seconds;
    };

    const onClickRecord = () => {
        if (/*!isConnected() || */!deviceFound())
            return;

        setRecordingStep(1);
        recordingTimerId = setInterval(() => {
            setElapsedTime(getElapsedTime() + 1);
        }, 1000);
        setInterval(sendChunkData, 100);

        mediaRecorder.start(captureInterval);
    };

    const onClickSave = () => {
        mediaRecorder.stop();
        clearInterval(recordingTimerId);
        setRecordingStep(2);
        setTimeout(onNotReceiveFinishMsg, 5000); // This is fallback when not received saved message from BE
    };

    const onNotReceiveFinishMsg = () => {
        if (chunks.length == 0 && isConnected() && getRecordingStep() != 3)
            playSavedVideo();
        else
            setTimeout(onNotReceiveFinishMsg, 1000);
    };

    const sendChunkData = () => {
        if (isConnected() && chunks.length > 0) {
            async_send(chunks.shift()!);
        }
    };

    const async_send = async (data: Blob) => {
        socket?.send(data);
        console.log('sending', data);
    };

    const playSavedVideo = () => {
        localVideo.srcObject = null;
        localVideo.setAttribute('src', baseUrl + "/static/uploads/" + getRecordingId() + ".webm");
        localVideo.setAttribute('controls', true);

        setRecordingStep(3);
    };

    return (
        <div class='container mx-auto mt-20'>
            <div class="flex">
                <div class='w-1/3 text-yellow-100'>
                    <h1 class='title'>Media Recorder</h1>

                    <h1>Connection: <Show when={isConnected()} fallback={<span class='text-red-500'>Failed</span>}>Ok</Show></h1>
                    <h5>Device: <Show when={deviceFound()} fallback={<span class='text-red-500'>Failed</span>}>Ok</Show></h5>
                    <h5>Status:
                        <Switch>
                            <Match when={getRecordingStep() == 0}>None</Match>
                            <Match when={getRecordingStep() == 1}>Recording</Match>
                            <Match when={getRecordingStep() == 2}>Saving...</Match>
                            <Match when={getRecordingStep() == 3}>Saved</Match>
                        </Switch>
                    </h5>

                    <h5>Elapsed: {formatElapsedTime()}</h5>

                    <div class='mb-[20px]'></div>

                    <Show when={getRecordingStep() == 0}>
                        <button class="button-85" onClick={onClickRecord}>Record</button>
                    </Show>

                    <Show when={getRecordingStep() == 1}>
                        <button class="button-85" onClick={onClickSave}>Save</button>
                    </Show>
                </div>
                <div class='w-2/3'>
                    <video id="localVideo" autoplay src=""></video>
                </div>
            </div>
        </div>
    );
};

export default RecordingScreen;
