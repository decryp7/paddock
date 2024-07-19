import React, {useEffect, useState} from "react";
import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/tauri'

const VersionPane = React.memo((props , context) =>{
    const [version, setVersion] = useState("");

    useEffect(()=>{
        invoke('get_latest_version').then((v: any) => setVersion(v));

        const unListen = listen<string>('latest-version', (event) => {
            console.log('Received event:', event.payload);
            setVersion(event.payload)
        });

        return () => {
            if(unListen){
                unListen.then(f => f());
            }
        };
    });

    function handleAcknowledge(){
        invoke('acknowledge', {version: version}).then(r => console.log(r));
    }

    return <div className="flex flex-col space-y-3">
        <div
            className="flex flex-col items-center justify-center block p-6 bg-white border border-gray-200 rounded-lg shadow hover:bg-gray-100 dark:bg-gray-800 dark:border-gray-700 dark:hover:bg-gray-700">
            <h1 className="text-[70px] font-extrabold">{version}</h1>
            <button type="button"
                    className="text-white bg-blue-700 hover:bg-blue-800 focus:ring-4 focus:ring-blue-300 font-medium rounded-lg text-sm px-5 py-2.5 me-2 mb-2 dark:bg-blue-600 dark:hover:bg-blue-700 focus:outline-none dark:focus:ring-blue-800"
                    onClick={handleAcknowledge}>Tolong eh! I know already! Can stop notifying or not?
            </button>
        </div>
    </div>;
});

VersionPane.displayName = "VersionPane";

export default VersionPane;