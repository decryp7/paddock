import React, {useEffect, useState} from "react";
import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/tauri'

const VersionPane = React.memo((props , context) =>{
    const [version, setVersion] = useState("R0.00.00T00");
    const [ack, setAck] = useState(true);
    const [autoLaunch, setAutoLaunch] = useState(false);

    useEffect(()=>{
        invoke('get_latest_version').then((v: any) => setVersion(v));
        invoke('get_auto_launch').then((b: any) => setAutoLaunch(b));
        invoke('get_acked', {version: version}).then((a: any) => setAck(a));

        const unListen = listen<string>('latest-version', (event) => {
            console.log('Received event:', event.payload);
            setVersion(event.payload);
        });

        return () => {
            if(unListen){
                unListen.then(f => f());
            }
        };
    });

    function handleAcknowledge(){
        invoke('acknowledge', {version: version}).then((r: any) =>{
            console.log(r);
            setAck(r);
        });
    }

    function handleChangeAutoLaunch(e : React.ChangeEvent<HTMLInputElement>)    {
        console.log(e.target.checked);
        invoke('set_auto_launch', {autoLaunch: e.target.checked})
            .then((b: any) => setAutoLaunch(b));
    }

    return <div className="flex flex-col space-y-3 text-gray-700 dark:text-white">
        <div
            className="flex flex-col items-center justify-center p-6 bg-white border border-gray-200 rounded-lg shadow dark:bg-gray-800 dark:border-gray-700">
            <div>The latest build version is</div>
            <h1 className="text-[80px] font-extrabold">{version}</h1>
            <button type="button"
                    className="disabled:bg-slate-200 disabled:text-slate-500 disabled:hover:bg-slate-50 text-white bg-blue-700 hover:bg-blue-800 focus:ring-4 focus:ring-blue-300 font-medium rounded-lg text-sm px-5 py-2.5 me-2 mb-2 dark:bg-blue-600 dark:hover:bg-blue-700 focus:outline-none dark:focus:ring-blue-800"
                    onClick={handleAcknowledge}
                    disabled={ack}>Tolong eh! I know already! Can stop notifying or not?</button>
        </div>
        <div className="w-full">
            <input type="checkbox"
                   checked={autoLaunch}
                   onChange={handleChangeAutoLaunch}
                   className="w-4 h-4 text-blue-600 bg-gray-100 border-gray-300 rounded focus:ring-blue-500 dark:focus:ring-blue-600 dark:ring-offset-gray-800 focus:ring-2 dark:bg-gray-700 dark:border-gray-600"/>
            <label className="ms-2 text-sm font-medium dark:text-gray-300">Please auto launch when PC start hor!</label>
        </div>
    </div>;
});

VersionPane.displayName = "VersionPane";

export default VersionPane;