use std::path::{Path, PathBuf};
use std::process::{Command, exit};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about, long_about = "Deploy dll for exe or dll.")]
struct Args {
    /// The target file to deploy dll for. This can be an exe or dll.
    binary_file: String,

    /// Do not search in system variable PATH
    #[arg(long, default_value_t = false)]
    skip_env_path: bool,

    /// Copy Microsoft Visual C/C++ redistributable dlls.
    #[arg(long, default_value_t = false)]
    copy_vc_redist: bool,

    /// Show verbose information during execution
    #[arg(long, default_value_t = false)]
    verbose: bool,

    /// Search for dll in those dirs
    #[arg(long)]
    shallow_search_dir: Vec<String>,
    /// Disable shallow search
    #[arg(long, default_value_t = false)]
    no_shallow_search: bool,

    /// Search for dll recursively in those dirs
    #[arg(long)]
    deep_search_dir: Vec<String>,
    /// Disable recursive search
    #[arg(long, default_value_t = false)]
    no_deep_search: bool,

    /// CMAKE_PREFIX_PATH for cmake to search for packages
    #[arg(long)]
    cmake_prefix_path: Vec<String>,
    /// Dll files that won't be deployed
    #[arg(long)]
    ignore: Vec<String>,

    /// Location of dumpbin file. Valid values: [auto] [system] [builtin] path
    #[arg(long, default_value_t = String::from("[auto]"))]
    objdump_file: String,
    /// If one or more dll failed to be found, skip it and go on
    #[arg(long, default_value_t = false)]
    allow_missing: bool,
}

fn existing_var_path(dest: &mut Vec<String>) {
    if let Ok(path) = std::env::var("PATH") {
        for path in path.split(';') {
            if !can_be_dir(&path) {
                continue;
            }
            dest.push(path.to_string());
        }
    }
}

fn get_system_objdump()->Option<String> {
    let output=Command::new("where").args(["objdump"]).output().unwrap();
    let output=String::from_utf8(output.stdout).unwrap().replace('\r',"");
    for line in output.split('\n') {
        if is_file(&line) {
            return Some(line.to_string());
        }
    }
    return None;
}

fn get_objdump_file(input:&str)->String {
    if input=="[system]" {
        if let Some(loc)=get_system_objdump() {
            return loc;
        }
        eprintln!("Failed to find objdump in your system");
        exit(2);
    }

    if input == "[builtin]" {
        let current_exe = std::env::current_exe().expect("Get current exe name");
        //println!("current_exe = {}",current_exe.to_str().unwrap());
        let install_prefix = current_exe.parent().expect("Get parent dir of current exe");
        //println!("install_prefix = {}",install_prefix.to_str().unwrap());
        let mut p = install_prefix.to_path_buf();
        p.push("objdump.exe");

        if !is_file(&p) {
            eprintln!("Builtin objdump executable {} not found",p.display());
            exit(3);
        }

        //println!("objdump path = {}",p.to_str().unwrap());
        return p.to_str().unwrap().to_string();
    }

    if input=="[auto]"  {
        if let Some(loc)=get_system_objdump() {
            return loc;
        }
        return get_objdump_file("[builtin]");
    }

    if !is_file(&input) {
        eprintln!("Given objdump file {} doesn't exist",input);
        exit(4);
    }

    return input.to_string();
}

impl Args {
    fn objdump_file(&self) -> String {
        return get_objdump_file(&self.objdump_file);
    }

    fn shallow_search_dirs(&self) -> Vec<String> {
        let mut vec = self.shallow_search_dir.clone();
        self.existing_cmake_prefix_path(&mut vec);

        if cfg!(target_os = "windows") && !self.skip_env_path {
            existing_var_path(&mut vec);
        }

        return vec;
    }

    fn existing_cmake_prefix_path(&self, dest: &mut Vec<String>) {
        for path in &self.cmake_prefix_path {
            for path in path.split(';') {
                let path = format!("{path}/bin");
                if can_be_dir(&path) {
                    dest.push(path);
                }
            }
        }
    }

    fn deep_search_dirs(&self) -> Vec<String> {
        let mut vec = self.deep_search_dir.clone();
        self.existing_cmake_prefix_path(&mut vec);

        if cfg!(target_os = "windows") && !self.skip_env_path {
            existing_var_path(&mut vec);
        }

        return vec;
    }
}

fn parse_output_single_line(output: &str) -> &str {
    let fail_msg = format!("Failed to parse dll name from output {output}");
    let loc1 = output.find("DLL Name: ").expect(&fail_msg);
    let loc2 = output.find(".dll").expect(&fail_msg);

    let loc1 = loc1 + "DLL Name: ".len();
    if loc1 + 1 >= loc2 {
        eprintln!("{}", fail_msg);
        exit(8);
    }

    return &output[loc1..loc2];
}

fn get_dependencies(file: &str, objdump_file: &str) -> Vec<String> {
    let output = Command::new(objdump_file).args([file, "-x", "--section=.rdata"]).output()
        .expect(&format!("Failed to run objdump at {}", objdump_file));

    if !output.status.success() {
        eprintln!("{} {} -x failed with error code {}", objdump_file, file, output.status.to_string());
        eprintln!("The std error is: {}", String::from_utf8(output.stderr).unwrap());
        exit(1);
    }

    let output = String::from_utf8(output.stdout).expect("Failed to convert output to utf8");
    let split = output.split("\n");
    let mut dlls = Vec::with_capacity(split.clone().count());
    //let regex=Regex::new(r"DLL Name: (.+)\.dll").unwrap();
    for line in split {
        if !line.contains("DLL Name:") {
            continue;
        }

        let mut str = parse_output_single_line(line).to_string();
        str.push_str(".dll");
        dlls.push(str);
    }
    return dlls;
}

fn is_vc_redist_dll(name: &str) -> bool {
    return name.starts_with("api-ms-win");
}

fn is_system_dll(name: &str) -> bool {
    return if cfg!(target_os = "windows") {
        let system_prefices = [
            "C:/Windows/",
            "C:/Windows/system32/",
            "C:/Windows/System32/Wbem/",
            "C:/Windows/System32/WindowsPowerShell/v1.0/",
            "C:/Windows/System32/OpenSSH/"];
        for prefix in system_prefices {
            let filename = format!("{prefix}{name}");
            if is_file(&filename) {
                return true;
            }
        }

        false
    } else {
        // Fallback solution for cross compiling
        const SYSTEM_DLL_LIST: [&str; 3393] = ["07409496-a423-4a3e-b620-2cfb01a9318d_HyperV-ComputeNetwork.dll", "0ae3b998-9a38-4b72-a4c4-06849441518d_Servicing-Stack.dll", "4545ffe2-0dc4-4df4-9d02-299ef204635e_hvsocket.dll", "69fe178f-26e7-43a9-aa7d-2b616b672dde_eventlogservice.dll", "6bea57fb-8dfb-4177-9ae8-42e8b3529933_RuntimeDeviceInstall.dll", "aadauthhelper.dll", "aadcloudap.dll", "aadjcsp.dll", "aadtb.dll", "aadWamExtension.dll", "AarSvc.dll", "AboutSettingsHandlers.dll", "AboveLockAppHost.dll", "accessibilitycpl.dll", "accountaccessor.dll", "AccountsRt.dll", "AcGenral.dll", "AcLayers.dll", "acledit.dll", "aclui.dll", "acmigration.dll", "ACPBackgroundManagerPolicy.dll", "acppage.dll", "acproxy.dll", "AcSpecfc.dll", "ActionCenter.dll", "ActionCenterCPL.dll", "ActionQueue.dll", "ActivationClient.dll", "ActivationManager.dll", "activeds.dll", "ActiveSyncCsp.dll", "ActiveSyncProvider.dll", "actxprxy.dll", "AcWinRT.dll", "AcXtrnal.dll", "adal.dll", "AdaptiveCards.dll", "AddressParser.dll", "adhapi.dll", "adhsvc.dll", "AdmTmpl.dll", "admwprox.dll", "AdobePDF.dll", "AdobePDFUI.dll", "adprovider.dll", "adsldp.dll", "adsldpc.dll", "adsmsext.dll", "adsnt.dll", "adtschema.dll", "AdvancedEmojiDS.dll", "advapi32.dll", "advapi32res.dll", "advpack.dll", "aeevts.dll", "aeinv.dll", "aemarebackup.dll", "aepic.dll", "agentactivationruntime.dll", "agentactivationruntimewindows.dll", "ahadmin.dll", "AJRouter.dll", "amsi.dll", "amsiproxy.dll", "amstream.dll", "Analog.Shell.Broker.dll", "AnalogCommonProxyStub.dll", "apds.dll", "APHostClient.dll", "APHostRes.dll", "APHostService.dll", "apisampling.dll", "ApiSetHost.AppExecutionAlias.dll", "apisetschema.dll", "APMon.dll", "APMonUI.dll", "AppContracts.dll", "AppExtension.dll", "apphelp.dll", "Apphlpdm.dll", "appidapi.dll", "AppIdPolicyEngineApi.dll", "appidsvc.dll", "appinfo.dll", "appinfoext.dll", "AppInstallerPrompt.Desktop.dll", "ApplicationControlCSP.dll", "ApplicationFrame.dll", "ApplicationTargetedFeatureDatabase.dll", "AppListBackupLauncher.dll", "AppLockerCSP.dll", "appmgmts.dll", "appmgr.dll", "AppMon.dll", "AppointmentActivation.dll", "AppointmentApis.dll", "appraiser.dll", "AppReadiness.dll", "apprepapi.dll", "AppResolver.dll", "appsruprov.dll", "appverifUI.dll", "AppxAllUserStore.dll", "AppXApplicabilityBlob.dll", "AppxApplicabilityEngine.dll", "AppXDeploymentClient.dll", "AppXDeploymentExtensions.desktop.dll", "AppXDeploymentExtensions.onecore.dll", "AppXDeploymentServer.dll", "AppxPackaging.dll", "AppxSip.dll", "AppxStreamingDataSourcePS.dll", "AppxSysprep.dll", "Apx01000.dll", "archiveint.dll", "asferror.dll", "aspnet_counters.dll", "aspperf.dll", "AssignedAccessRuntime.dll", "asycfilt.dll", "atl.dll", "atl100.dll", "atl110.dll", "atlthunk.dll", "atmlib.dll", "AttestationWmiProvider.dll", "AudioEndpointBuilder.dll", "AudioEng.dll", "AudioHandlers.dll", "AUDIOKSE.dll", "audioresourceregistrar.dll", "AudioSes.dll", "audiosrv.dll", "AudioSrvPolicyManager.dll", "auditcse.dll", "AuditNativeSnapIn.dll", "auditpolcore.dll", "AuditPolicyGPInterop.dll", "auditpolmsg.dll", "AuthBroker.dll", "AuthBrokerUI.dll", "authentication.dll", "AuthExt.dll", "authfwcfg.dll", "AuthFWGP.dll", "AuthFWSnapin.dll", "AuthFWWizFwk.dll", "AuthHostProxy.dll", "authui.dll", "authz.dll", "AutomaticAppSignInPolicy.dll", "autopilot.dll", "autopilotdiag.dll", "autoplay.dll", "autotimesvc.dll", "avicap32.dll", "avifil32.dll", "avrt.dll", "AxInstSv.dll", "azroles.dll", "azroleui.dll", "AzSqlExt.dll", "BackgroundMediaPolicy.dll", "BamSettingsClient.dll", "BarcodeProvisioningPlugin.dll", "basecsp.dll", "basesrv.dll", "batmeter.dll", "bcastdvr.proxy.dll", "BcastDVRBroker.dll", "BcastDVRClient.dll", "BcastDVRCommon.dll", "bcastdvruserservice.dll", "bcd.dll", "bcdprov.dll", "bcdsrv.dll", "BCP47Langs.dll", "BCP47mrm.dll", "bcrypt.dll", "bcryptprimitives.dll", "BdeHdCfgLib.dll", "bderepair.dll", "bdesvc.dll", "bdeui.dll", "bi.dll", "bidispl.dll", "bindfltapi.dll", "BingASDS.dll", "BingFilterDS.dll", "BingMaps.dll", "BingOnlineServices.dll", "BioCredProv.dll", "bisrv.dll", "BitLockerCsp.dll", "bitsigd.dll", "bitsperf.dll", "BitsProxy.dll", "biwinrt.dll", "BlbEvents.dll", "blbres.dll", "blb_ps.dll", "BluetoothApis.dll", "BluetoothDesktopHandlers.dll", "BluetoothOppPushClient.dll", "bnmanager.dll", "BootMenuUX.dll", "bootstr.dll", "bootsvc.dll", "bootux.dll", "bridgeres.dll", "BrokerFileDialog.dll", "BrokerLib.dll", "browcli.dll", "browser.dll", "browserbroker.dll", "browseui.dll", "BTAGService.dll", "BthAvctpSvc.dll", "BthAvrcp.dll", "BthAvrcpAppSvc.dll", "bthci.dll", "BthMtpContextHandler.dll", "bthpanapi.dll", "BthpanContextHandler.dll", "BthRadioMedia.dll", "bthserv.dll", "BthTelemetry.dll", "btpanui.dll", "BWContextHandler.dll", "c4d66f00-b6f0-4439-ac9b-c5ea13fe54d7_HyperV-ComputeCore.dll", "cabapi.dll", "cabinet.dll", "cabview.dll", "CallButtons.dll", "CallButtons.ProxyStub.dll", "CallHistoryClient.dll", "CameraCaptureUI.dll", "camext.dll", "CapabilityAccessHandlers.dll", "CapabilityAccessManager.dll", "CapabilityAccessManagerClient.dll", "capauthz.dll", "capiprovider.dll", "capisp.dll", "CaptureService.dll", "CastingShellExt.dll", "CastLaunch.dll", "catsrv.dll", "catsrvps.dll", "catsrvut.dll", "CBDHSvc.dll", "cca.dll", "cdd.dll", "cdosys.dll", "cdp.dll", "cdprt.dll", "cdpsvc.dll", "cdpusersvc.dll", "cellulardatacapabilityhandler.dll", "cemapi.dll", "certca.dll", "certcli.dll", "certCredProvider.dll", "certenc.dll", "CertEnroll.dll", "CertEnrollUI.dll", "certmgr.dll", "CertPKICmdlet.dll", "CertPolEng.dll", "certprop.dll", "cewmdm.dll", "cfgbkend.dll", "cfgmgr32.dll", "CfgSPCellular.dll", "CfgSPPolicy.dll", "cflapi.dll", "cfmifs.dll", "cfmifsproxy.dll", "Chakra.dll", "Chakradiag.dll", "Chakrathunk.dll", "chartv.dll", "ChatApis.dll", "ChsStrokeDS.dll", "ChtBopomofoDS.dll", "ChtCangjieDS.dll", "ChtHkStrokeDS.dll", "ChtQuickDS.dll", "ChxAPDS.dll", "ChxDecoder.dll", "ChxHAPDS.dll", "chxinputrouter.dll", "chxranker.dll", "CHxReadingStringIME.dll", "ci.dll", "cic.dll", "cimfs.dll", "CIRCoInst.dll", "clbcatq.dll", "cldapi.dll", "CleanPCCSP.dll", "clfsw32.dll", "cliconfg.dll", "ClipboardServer.dll", "Clipc.dll", "ClipSVC.dll", "clipwinrt.dll", "cloudAP.dll", "CloudDesktopCSP.dll", "CloudDomainJoinAUG.dll", "CloudDomainJoinDataModelServer.dll", "CloudExperienceHost.dll", "CloudExperienceHostBroker.dll", "CloudExperienceHostCommon.dll", "CloudExperienceHostRedirection.dll", "CloudExperienceHostUser.dll", "CloudIdWxhExtension.dll", "CloudRecoveryDownloadTool.dll", "CloudRestoreLauncher.dll", "clrhost.dll", "clusapi.dll", "cmcfg32.dll", "cmdext.dll", "cmdial32.dll", "cmgrcspps.dll", "cmifw.dll", "cmintegrator.dll", "cmlua.dll", "cmpbk32.dll", "cmstplua.dll", "cmutil.dll", "cngcredui.dll", "cngkeyhelper.dll", "cngprovider.dll", "cnvfat.dll", "CodeIntegrityAggregator.dll", "cofiredm.dll", "colbact.dll", "colorui.dll", "combase.dll", "comcat.dll", "comctl32.dll", "comdlg32.dll", "coml2.dll", "CompatAggregator.dll", "ComposableShellProxyStub.dll", "ComposerFramework.dll", "CompPkgSup.dll", "compstui.dll", "computecore.dll", "computelibeventlog.dll", "computenetwork.dll", "computestorage.dll", "comrepl.dll", "comres.dll", "comsnap.dll", "comsvcs.dll", "comuid.dll", "concrt140.dll", "concrt140d.dll", "configmanager2.dll", "ConfigureExpandedStorage.dll", "ConhostV1.dll", "connect.dll", "ConnectedAccountState.dll", "ConsentExperienceCommon.dll", "ConsentUX.dll", "ConsentUxClient.dll", "console.dll", "ConsoleLogon.dll", "ConstraintIndex.Search.dll", "ContactActivation.dll", "ContactApis.dll", "ContactHarvesterDS.dll", "container.dll", "containerdevicemanagement.dll", "ContentDeliveryManager.Utilities.dll", "ControlLib.dll", "coreaudiopolicymanagerext.dll", "coredpus.dll", "coreglobconfig.dll", "CoreMas.dll", "CoreMessaging.dll", "CoreMmRes.dll", "CorePrivacySettingsStore.dll", "CoreShell.dll", "CoreShellAPI.dll", "CoreShellExtFramework.dll", "CoreUIComponents.dll", "correngine.dll", "CourtesyEngine.dll", "CPFilters.dll", "CredDialogBroker.dll", "CredentialEnrollmentManagerForUser.dll", "CredProv2faHelper.dll", "CredProvCommonCore.dll", "CredProvDataModel.dll", "CredProvHelper.dll", "credprovhost.dll", "credprovs.dll", "credprovslegacy.dll", "credssp.dll", "credui.dll", "crypt32.dll", "cryptbase.dll", "cryptcatsvc.dll", "cryptdlg.dll", "cryptdll.dll", "cryptext.dll", "cryptnet.dll", "cryptngc.dll", "CryptoWinRT.dll", "cryptsp.dll", "cryptsvc.dll", "crypttpmeksvc.dll", "cryptui.dll", "cryptuiwizard.dll", "cryptxml.dll", "cscapi.dll", "cscdll.dll", "CspCellularSettings.dll", "csplte.dll", "CspProxy.dll", "csrsrv.dll", "CSystemEventsBrokerClient.dll", "cuzzapi.dll", "cxcredprov.dll", "CXHProvisioningServer.dll", "d2d1.dll", "d2d1debug3.dll", "d3d10.dll", "d3d10core.dll", "d3d10level9.dll", "d3d10ref.dll", "d3d10sdklayers.dll", "d3d10warp.dll", "d3d10_1.dll", "d3d10_1core.dll", "d3d11.dll", "d3d11on12.dll", "d3d11_3SDKLayers.dll", "D3D12.dll", "D3D12Core.dll", "d3d12SDKLayers.dll", "d3d8thk.dll", "d3d9.dll", "d3d9on12.dll", "D3DCompiler_43.dll", "D3DCompiler_47.dll", "d3dcsx_43.dll", "d3dref9.dll", "D3DSCache.dll", "d3dx10_43.dll", "d3dx11_43.dll", "d3dx9_30.dll", "D3DX9_43.dll", "d4d78066-e6db-44b7-b5cd-2eb82dce620c_HyperV-ComputeLegacy.dll", "dab.dll", "dabapi.dll", "DAConn.dll", "dafAspInfraProvider.dll", "dafBth.dll", "DafDnsSd.dll", "dafDockingProvider.dll", "DAFESCL.dll", "DafGip.dll", "DAFIoT.dll", "DAFIPP.dll", "DAFMCP.dll", "dafpos.dll", "DafPrintProvider.dll", "dafupnp.dll", "dafWCN.dll", "dafWfdProvider.dll", "DAFWiProv.dll", "DAFWSD.dll", "DAMediaManager.dll", "DAMM.dll", "DaOtpCredentialProvider.dll", "das.dll", "dataclen.dll", "DataExchange.dll", "datusage.dll", "davclnt.dll", "davhlpr.dll", "DavSyncProvider.dll", "daxexec.dll", "dbgcore.dll", "dbgeng.dll", "dbghelp.dll", "DbgModel.dll", "dbnetlib.dll", "dbnmpntw.dll", "dciman32.dll", "dcntel.dll", "dcomp.dll", "dcsvc.dll", "DDACLSys.dll", "DdcClaimsApi.dll", "DdcComImplementationsDesktop.dll", "DDDS.dll", "ddisplay.dll", "DDOIProxy.dll", "DDORes.dll", "ddraw.dll", "ddrawex.dll", "declaredconfiguration.dll", "DefaultDeviceManager.dll", "DefaultPrinterProvider.dll", "defragproxy.dll", "defragres.dll", "defragsvc.dll", "delegatorprovider.dll", "deploymentcsps.dll", "deskadp.dll", "deskmon.dll", "DesktopShellAppStateContract.dll", "DesktopShellExt.dll", "DesktopSwitcherDataModel.dll", "DesktopView.Internal.Broker.dll", "DesktopView.Internal.Broker.ProxyStub.dll", "DevDispItemProvider.dll", "DeveloperOptionsSettingsHandlers.dll", "devenum.dll", "deviceaccess.dll", "deviceassociation.dll", "DeviceCenter.dll", "DeviceCompanionAppInstall.dll", "DeviceCredential.dll", "DeviceDirectoryClient.dll", "DeviceDisplayStatusManager.dll", "DeviceDriverRetrievalClient.dll", "DeviceElementSource.dll", "DeviceFlows.DataModel.dll", "DeviceMetadataRetrievalClient.dll", "devicengccredprov.dll", "DevicePairing.dll", "DevicePairingExperienceMEM.dll", "DevicePairingFolder.dll", "DevicePairingProxy.dll", "DeviceReactivation.dll", "deviceregistration.dll", "DeviceSetupManager.dll", "DeviceSetupManagerAPI.dll", "DeviceSetupStatusProvider.dll", "DevicesFlowBroker.dll", "DeviceSoftwareInstallationClient.dll", "DeviceUpdateAgent.dll", "DeviceUxRes.dll", "devinv.dll", "devmgr.dll", "devobj.dll", "DevPropMgr.dll", "DevQueryBroker.dll", "devrtl.dll", "dfdts.dll", "dfscli.dll", "dfshim.dll", "DfsShlEx.dll", "dhcpcmonitor.dll", "dhcpcore.dll", "dhcpcore6.dll", "dhcpcsvc.dll", "dhcpcsvc6.dll", "dhcpsapi.dll", "DHolographicDisplay.dll", "DiagCpl.dll", "diagnosticdataquery.dll", "DiagnosticDataSettings.dll", "DiagnosticInvoker.dll", "DiagnosticLogCSP.dll", "diagperf.dll", "DiagSvc.dll", "diagtrack.dll", "dialclient.dll", "dialserver.dll", "DictationManager.dll", "difxapi.dll", "dimsjob.dll", "dimsroam.dll", "dinput.dll", "dinput8.dll", "Direct2DDesktop.dll", "directmanipulation.dll", "DirectML.Debug.dll", "directml.dll", "directxdatabasehelper.dll", "discan.dll", "DismApi.dll", "DispBroker.Desktop.dll", "DispBroker.dll", "dispex.dll", "Display.dll", "DisplayManager.dll", "dlnashext.dll", "DMAlertListener.ProxyStub.dll", "DmApiSetExtImplDesktop.dll", "DMAppsRes.dll", "dmcfgutils.dll", "dmcmnutils.dll", "dmcommandlineutils.dll", "dmcsps.dll", "dmdlgs.dll", "dmdskmgr.dll", "dmdskres.dll", "dmdskres2.dll", "dmenrollengine.dll", "dmenterprisediagnostics.dll", "dmintf.dll", "dmiso8601utils.dll", "dmloader.dll", "dmocx.dll", "dmoleaututils.dll", "dmprocessxmlfiltered.dll", "dmpushproxy.dll", "DMPushRouterCore.dll", "DMRCDecoder.dll", "DMRServer.dll", "dmsynth.dll", "dmusic.dll", "dmutil.dll", "dmvdsitf.dll", "dmwappushsvc.dll", "dmwmicsp.dll", "dmxmlhelputils.dll", "dnsapi.dll", "dnscmmc.dll", "dnsext.dll", "dnshc.dll", "dnsrslvr.dll", "Docking.VirtualInput.dll", "DockInterface.ProxyStub.dll", "doclient.dll", "docprop.dll", "DocumentPerformanceEvents.dll", "DolbyDecMFT.dll", "domgmt.dll", "domiprov.dll", "dosettings.dll", "dosvc.dll", "dot3api.dll", "dot3cfg.dll", "Dot3Conn.dll", "dot3dlg.dll", "dot3gpclnt.dll", "dot3gpui.dll", "dot3hc.dll", "dot3mm.dll", "dot3msm.dll", "dot3svc.dll", "dot3ui.dll", "dpapi.dll", "dpapiprovider.dll", "dpapisrv.dll", "dplcsp.dll", "dpnaddr.dll", "dpnathlp.dll", "dpnet.dll", "dpnhpast.dll", "dpnhupnp.dll", "dpnlobby.dll", "dps.dll", "dpx.dll", "DragDropExperienceCommon.dll", "DragDropExperienceDataExchangeDelegated.dll", "drprov.dll", "drt.dll", "drtprov.dll", "drttransport.dll", "drvsetup.dll", "drvstore.dll", "dsauth.dll", "DscCore.dll", "DscCoreConfProv.dll", "dsclient.dll", "dscproxy.dll", "DscTimer.dll", "dsdmo.dll", "dskquota.dll", "dskquoui.dll", "dsound.dll", "dsparse.dll", "dsprop.dll", "dsquery.dll", "dsreg.dll", "dsregtask.dll", "dsrole.dll", "dssec.dll", "dssenh.dll", "dssvc.dll", "Dsui.dll", "dsuiext.dll", "dswave.dll", "dtsh.dll", "DTSPipelinePerf150.dll", "DuCsps.dll", "dui70.dll", "duser.dll", "dusmapi.dll", "dusmsvc.dll", "dwmapi.dll", "dwmcore.dll", "dwmghost.dll", "dwminit.dll", "dwmredir.dll", "dwmscene.dll", "DWrite.dll", "DXCaptureReplay.dll", "DXCore.dll", "dxdiagn.dll", "dxgi.dll", "DXGIDebug.dll", "dxgwdi.dll", "dxilconv.dll", "dxmasf.dll", "DXP.dll", "dxpps.dll", "DxpTaskSync.dll", "dxtmsft.dll", "DXToolsMonitor.dll", "DXToolsOfflineAnalysis.dll", "DxToolsReportGenerator.dll", "DXToolsReporting.dll", "dxtrans.dll", "dxva2.dll", "dynamoapi.dll", "EAMProgressHandler.dll", "eapp3hst.dll", "eappcfg.dll", "eappcfgui.dll", "eappgnui.dll", "eapphost.dll", "eappprxy.dll", "eapprovp.dll", "eapputil.dll", "eapsimextdesktop.dll", "eapsvc.dll", "EapTeapAuth.dll", "EapTeapConfig.dll", "EapTeapExt.dll", "easconsent.dll", "easinvoker.proxystub.dll", "EasPolicyManagerBrokerPS.dll", "easwrt.dll", "edgeangle.dll", "EdgeContent.dll", "edgehtml.dll", "edgeIso.dll", "EdgeManager.dll", "EdgeResetPlugin.dll", "EditBufferTestHook.dll", "EditionUpgradeHelper.dll", "EditionUpgradeManagerObj.dll", "edpauditapi.dll", "edpcsp.dll", "edptask.dll", "edputil.dll", "eeprov.dll", "eeutil.dll", "efsadu.dll", "efscore.dll", "efsext.dll", "efslsaext.dll", "efssvc.dll", "efsutil.dll", "efswrt.dll", "EhStorAPI.dll", "EhStorPwdMgr.dll", "EhStorShell.dll", "ElevocDAPO.dll", "ElevocDNSEngine.dll", "ElevocGNA.dll", "ElevocKWSApo.dll", "ElevocSEEngine.dll", "ElevocUAPO.dll", "ElevocUNSEngine.dll", "elevoc_kws_engine.dll", "elevoc_speech_engine.dll", "elevoc_teams_aec.dll", "elevoc_voice_separation.dll", "els.dll", "ELSCore.dll", "elshyph.dll", "elslad.dll", "elsTrans.dll", "EmailApis.dll", "embeddedmodesvc.dll", "embeddedmodesvcapi.dll", "EmojiDS.dll", "encapi.dll", "Enclave_IOC.signed.dll", "Enclave_SSL.signed.dll", "energy.dll", "energyprov.dll", "energytask.dll", "enrollmentapi.dll", "EnterpriseAPNCsp.dll", "EnterpriseAppMgmtClient.dll", "EnterpriseAppMgmtSvc.dll", "enterprisecsps.dll", "EnterpriseDesktopAppMgmtCSP.dll", "enterpriseetw.dll", "EnterpriseModernAppMgmtCSP.dll", "enterpriseresourcemanager.dll", "eqossnap.dll", "ErrorDetails.dll", "ErrorDetailsCore.dll", "es.dll", "EsclProtocol.dll", "EsclScan.dll", "EsclWiaDriver.dll", "EsdSip.dll", "esent.dll", "esentprf.dll", "esevss.dll", "eShims.dll", "EthernetMediaManager.dll", "ETWCoreUIComponentsResources.dll", "ETWESEProviderResources.dll", "EtwRundown.dll", "eUICCsCSP.dll", "EventAggregation.dll", "eventcls.dll", "evr.dll", "ExecModelClient.dll", "execmodelproxy.dll", "ExplorerFrame.dll", "ExSMime.dll", "ExtrasXmlParser.dll", "f1db7d81-95be-4911-935a-8ab71629112a_HyperV-IsolatedVM.dll", "f3ahvoas.dll", "f989b52d-f928-44a3-9bf1-bf0c1da6a0d6_HyperV-DeviceVirtualization.dll", "facecredentialprovider.dll", "Face_Beauty_DLL_X64.dll", "Facilitator.dll", "Family.Authentication.dll", "Family.Cache.dll", "Family.Client.dll", "Family.SyncEngine.dll", "FamilySafetyExt.dll", "Faultrep.dll", "FaxPrinterInstaller.dll", "fcon.dll", "FCStdThumbnail.dll", "fdBth.dll", "fdBthProxy.dll", "FdDevQuery.dll", "fde.dll", "fdeploy.dll", "fdPHost.dll", "fdPnp.dll", "fdprint.dll", "fdProxy.dll", "FDResPub.dll", "fdSSDP.dll", "fdWCN.dll", "fdWNet.dll", "fdWSD.dll", "feclient.dll", "ffbroker.dll", "fhcat.dll", "fhcfg.dll", "fhcleanup.dll", "fhcpl.dll", "fhengine.dll", "fhevents.dll", "fhsettingsprovider.dll", "fhshl.dll", "fhsrchapi.dll", "fhsrchph.dll", "fhsvc.dll", "fhsvcctl.dll", "fhtask.dll", "fhuxadapter.dll", "fhuxapi.dll", "fhuxcommon.dll", "fhuxgraphics.dll", "fhuxpresentation.dll", "fidocredprov.dll", "FileAppxStreamingDataSource.dll", "filemgmt.dll", "FilterDS.dll", "findnetprinters.dll", "fingerprintcredential.dll", "FirewallAPI.dll", "FirewallControlPanel.dll", "FirewallUX.dll", "FirmwareAttestationServerProxyStub.dll", "FlightSettings.dll", "fltLib.dll", "FluencyDS.dll", "fmapi.dll", "fmifs.dll", "FMMP.dll", "fms.dll", "FntCache.dll", "fontext.dll", "FontGlyphAnimator.dll", "fontgroupsoverride.dll", "FontProvider.dll", "fontsub.dll", "fphc.dll", "framedyn.dll", "framedynos.dll", "FrameServer.dll", "FrameServerClient.dll", "FrameServerMonitor.dll", "FrameServerMonitorClient.dll", "frprov.dll", "FsNVSDeviceSource.dll", "fssres.dll", "fsutilext.dll", "fthsvc.dll", "fundisc.dll", "fveapi.dll", "fveapibase.dll", "fvecerts.dll", "fvecpl.dll", "fveskybackup.dll", "fveui.dll", "fvewiz.dll", "FvSDK_x64.dll", "fwbase.dll", "fwcfg.dll", "fwmdmcsp.dll", "fwpolicyiomgr.dll", "FwRemoteSvr.dll", "FXSAPI.dll", "FXSCOM.dll", "FXSCOMEX.dll", "FXSCOMPOSE.dll", "FXSCOMPOSERES.dll", "FXSEVENT.dll", "FXSMON.dll", "FXSRESM.dll", "FXSROUTE.dll", "FXSST.dll", "FXST30.dll", "FXSTIFF.dll", "FXSUTILITY.dll", "GameBarPresenceWriter.proxy.dll", "GameChatOverlayExt.dll", "GameChatTranscription.dll", "gameconfighelper.dll", "GameInput.dll", "GameInputInbox.dll", "GameInputRedist.dll", "gamelaunchhelper.dll", "gamemode.dll", "GamePanelExternalHook.dll", "gameplatformservices.dll", "gamestreamingext.dll", "gameux.dll", "gamingservicesproxy_4.dll", "gamingtcui.dll", "gamingtcuihelpers.dll", "gcdef.dll", "gdi32.dll", "gdi32full.dll", "GdiPlus.dll", "generaltel.dll", "Geocommon.dll", "Geolocation.dll", "getuname.dll", "glmf32.dll", "globinputhost.dll", "glu32.dll", "gmsaclient.dll", "gna.dll", "GNAPlugin.dll", "gpapi.dll", "GPCSEWrapperCsp.dll", "gpedit.dll", "gpprefcl.dll", "gpprnext.dll", "gpscript.dll", "gpsvc.dll", "gptext.dll", "gpupvdev.dll", "GraphicsCapture.dll", "GraphicsPerfSvc.dll", "Groupinghc.dll", "hadrres.dll", "hal.dll", "HalExtIntcLpioDMA.dll", "HalExtIntcPseDMA.dll", "HalExtPL080.dll", "HanjaDS.dll", "hascsp.dll", "HashtagDS.dll", "haspsrm_win64.dll", "hbaapi.dll", "hcproviders.dll", "HdcpHandler.dll", "HeatCore.dll", "HelpPaneProxy.dll", "hgattest.dll", "hgclientservice.dll", "hgclientserviceps.dll", "hgcpl.dll", "hgsclientplugin.dll", "HgsClientWmi.dll", "hhsetup.dll", "hid.dll", "HidCfu.dll", "hidserv.dll", "hlink.dll", "hmkd.dll", "hnetcfg.dll", "HNetCfgClient.dll", "hnetmon.dll", "hnsproxy.dll", "HologramCompositor.dll", "HologramWorld.dll", "HolographicExtensions.dll", "HolographicRuntimes.dll", "HoloShellRuntime.dll", "HoloSHExtensions.dll", "HoloSI.PCShell.dll", "HostGuardianServiceClientResources.dll", "HostNetSvc.dll", "hotplug.dll", "HrtfApo.dll", "HrtfDspCpu.dll", "hspapi.dll", "hspfw.dll", "httpapi.dll", "httpprxc.dll", "httpprxm.dll", "httpprxp.dll", "HttpsDataSource.dll", "htui.dll", "hvhostsvc.dll", "hvloader.dll", "HvSocket.dll", "hwreqchk.dll", "Hydrogen.dll", "HyperVSysprepProvider.dll", "IA2ComProxy.dll", "ias.dll", "iasacct.dll", "iasads.dll", "iasdatastore.dll", "iashlpr.dll", "IasMigPlugin.dll", "iasnap.dll", "iaspolcy.dll", "iasrad.dll", "iasrecst.dll", "iassam.dll", "iassdo.dll", "iassvcs.dll", "icfupgd.dll", "icm32.dll", "icmp.dll", "icmui.dll", "IconCodecService.dll", "icsigd.dll", "icsvc.dll", "icsvcext.dll", "icsvcvss.dll", "icu.dll", "icuin.dll", "icuuc.dll", "IdCtrls.dll", "IDStore.dll", "IEAdvpack.dll", "ieapfltr.dll", "iedkcs32.dll", "ieframe.dll", "iemigplugin.dll", "iepeers.dll", "ieproxy.dll", "IEProxyDesktop.dll", "iernonce.dll", "iertutil.dll", "iesetup.dll", "iesysprep.dll", "ieui.dll", "ifmon.dll", "ifsutil.dll", "ifsutilx.dll", "igdDiag.dll", "IHDS.dll", "iisrstap.dll", "iisRtl.dll", "imagehlp.dll", "imageres.dll", "imagesp1.dll", "imapi.dll", "imapi2.dll", "imapi2fs.dll", "ime_textinputhelpers.dll", "imgutil.dll", "imm32.dll", "ImplatSetup.dll", "IndexedDbLegacy.dll", "inetcomm.dll", "inetmib1.dll", "inetpp.dll", "inetppui.dll", "INETRES.dll", "inference_engine.dll", "inference_engine_c_api.dll", "inference_engine_legacy.dll", "inference_engine_transformations.dll", "InkEd.dll", "InkObjCore.dll", "InprocLogger.dll", "input.dll", "InputCloudStore.dll", "InputController.dll", "InputHost.dll", "InputInjectionBroker.dll", "InputLocaleManager.dll", "InputService.dll", "InputSwitch.dll", "InputViewExperience.dll", "inseng.dll", "InstallService.dll", "InstallServiceTasks.dll", "IntelligentPwdlessTask.dll", "intel_gfx_api-x64.dll", "internetmail.dll", "InternetMailCsp.dll", "invagent.dll", "InventorySvc.dll", "iologmsg.dll", "IPELoggingDictationHelper.dll", "iphlpsvc.dll", "ipnathlp.dll", "IpNatHlpClient.dll", "IppCommon.dll", "IppCommonProxy.dll", "iprtprio.dll", "iprtrmgr.dll", "ipsecsnp.dll", "ipsmsnap.dll", "ipxlatcfg.dll", "iri.dll", "iscsicpl.dll", "iscsidsc.dll", "iscsied.dll", "iscsiexe.dll", "iscsilog.dll", "iscsium.dll", "iscsiwmi.dll", "iscsiwmiv2.dll", "ISM.dll", "itircl.dll", "itss.dll", "iuilp.dll", "iumbase.dll", "iumcrypt.dll", "iumdll.dll", "IumSdk.dll", "iyuv_32.dll", "JavaScriptCollectionAgent.dll", "JHI64.dll", "joinproviderol.dll", "joinutil.dll", "JpMapControl.dll", "jpndecoder.dll", "jpninputrouter.dll", "jpnranker.dll", "JpnServiceDS.dll", "jscript.dll", "jscript9.dll", "jscript9diag.dll", "jscript9Legacy.dll", "jsproxy.dll", "kbd101.dll", "kbd101a.dll", "kbd101b.dll", "kbd101c.dll", "kbd103.dll", "kbd106.dll", "kbd106n.dll", "kbdarmph.dll", "kbdarmty.dll", "kbdax2.dll", "kbdfar.dll", "kbdgeoer.dll", "kbdgeome.dll", "kbdgeooa.dll", "kbdgeoqw.dll", "kbdhebl3.dll", "kbdibm02.dll", "kbdlisub.dll", "kbdlisus.dll", "kbdlk41a.dll", "kbdnec.dll", "kbdnec95.dll", "kbdnecat.dll", "kbdnecnt.dll", "kbdnko.dll", "kbdphags.dll", "kd.dll", "kdcom.dll", "kdcpw.dll", "kdhvcom.dll", "kdnet.dll", "kdnet_uart16550.dll", "KdsCli.dll", "kdstub.dll", "kdusb.dll", "kd_02_10df.dll", "kd_02_10ec.dll", "kd_02_1137.dll", "kd_02_14e4.dll", "kd_02_15b3.dll", "kd_02_1969.dll", "kd_02_19a2.dll", "kd_02_1af4.dll", "kd_02_8086.dll", "kd_07_1415.dll", "kd_0C_8086.dll", "keepaliveprovider.dll", "KerbClientShared.dll", "kerberos.dll", "kernel.appcore.dll", "kernel32.dll", "KernelBase.dll", "KeyCredMgr.dll", "keyiso.dll", "keymgr.dll", "KeywordDetectorMsftSidAdapter.dll", "KnobsCore.dll", "KnobsCsp.dll", "ksuser.dll", "ktmw32.dll", "l2gpstore.dll", "l2nacp.dll", "L2SecHC.dll", "LangCleanupSysprepAction.dll", "LanguageComponentsInstaller.dll", "LanguageOverlayServer.dll", "LanguageOverlayUtil.dll", "LanguagePackDiskCleanup.dll", "LanguagePackManagementCSP.dll", "laps.dll", "lapscsp.dll", "LegacyNetUX.dll", "LegacySystemSettings.dll", "lfsvc.dll", "libcrypto.dll", "libmfxhw64.dll", "libomp140.x86_64.dll", "libomp140d.x86_64.dll", "libvpl.dll", "LicenseManager.dll", "LicenseManagerApi.dll", "LicenseManagerSvc.dll", "licenseprotection.dll", "LicensingCSP.dll", "LicensingDiagSpp.dll", "LicensingWinRT.dll", "licmgr10.dll", "linkinfo.dll", "lltdapi.dll", "lltdres.dll", "lltdsvc.dll", "lmhsvc.dll", "loadperf.dll", "localsec.dll", "localspl.dll", "localui.dll", "LocationApi.dll", "LocationFramework.dll", "LocationFrameworkInternalPS.dll", "LocationFrameworkPS.dll", "LocationWinPalMisc.dll", "LockAppBroker.dll", "LockController.dll", "LockHostingFramework.dll", "LockScreenContent.dll", "LockScreenContentHost.dll", "LockScreenData.dll", "loghours.dll", "logoncli.dll", "LogonController.dll", "lpasvc.dll", "lpk.dll", "lpksetupproxyserv.dll", "lsaadt.dll", "lsasrv.dll", "lsm.dll", "lsmproxy.dll", "luiapi.dll", "lxutil.dll", "lz32.dll", "Magnification.dll", "MaintenanceUI.dll", "ManageCI.dll", "MapConfiguration.dll", "MapControlCore.dll", "MapControlStringsRes.dll", "MapGeocoder.dll", "mapi32.dll", "mapistub.dll", "MapRouter.dll", "MapsBtSvc.dll", "MapsBtSvcProxy.dll", "MapsCSP.dll", "MapsStore.dll", "mapstoasttask.dll", "mapsupdatetask.dll", "MbaeApi.dll", "MbaeApiPublic.dll", "MBMediaManager.dll", "mbsmsapi.dll", "mbussdapi.dll", "MCCSEngineShared.dll", "MCCSPal.dll", "mciavi32.dll", "mcicda.dll", "mciqtz32.dll", "mciseq.dll", "mciwave.dll", "McpManagementProxy.dll", "McpManagementService.dll", "MCRecvSrc.dll", "mcupdate_AuthenticAMD.dll", "mcupdate_GenuineIntel.dll", "MdmCommon.dll", "MdmDiagnostics.dll", "mdminst.dll", "mdmlocalmanagement.dll", "mdmmigrator.dll", "mdmpostprocessevaluator.dll", "mdmregistration.dll", "MediaFoundation.DefaultPerceptionProvider.dll", "MediaFoundationAggregator.dll", "MemoryDiagnostic.dll", "MessagingDataModel2.dll", "MessagingService.dll", "mf.dll", "mf3216.dll", "mfAACEnc.dll", "mfasfsrcsnk.dll", "mfaudiocnv.dll", "mfc100.dll", "mfc100chs.dll", "mfc100cht.dll", "mfc100deu.dll", "mfc100enu.dll", "mfc100esn.dll", "mfc100fra.dll", "mfc100ita.dll", "mfc100jpn.dll", "mfc100kor.dll", "mfc100rus.dll", "mfc100u.dll", "mfc110.dll", "mfc110chs.dll", "mfc110cht.dll", "mfc110deu.dll", "mfc110enu.dll", "mfc110esn.dll", "mfc110fra.dll", "mfc110ita.dll", "mfc110jpn.dll", "mfc110kor.dll", "mfc110rus.dll", "mfc110u.dll", "mfc120.dll", "mfc120chs.dll", "mfc120cht.dll", "mfc120deu.dll", "mfc120enu.dll", "mfc120esn.dll", "mfc120fra.dll", "mfc120ita.dll", "mfc120jpn.dll", "mfc120kor.dll", "mfc120rus.dll", "mfc120u.dll", "mfc140.dll", "mfc140chs.dll", "mfc140cht.dll", "mfc140d.dll", "mfc140deu.dll", "mfc140enu.dll", "mfc140esn.dll", "mfc140fra.dll", "mfc140ita.dll", "mfc140jpn.dll", "mfc140kor.dll", "mfc140rus.dll", "mfc140u.dll", "mfc140ud.dll", "mfc42.dll", "mfc42u.dll", "MFCaptureEngine.dll", "mfcm100.dll", "mfcm100u.dll", "mfcm110.dll", "mfcm110u.dll", "mfcm120.dll", "mfcm120u.dll", "mfcm140.dll", "mfcm140d.dll", "mfcm140u.dll", "mfcm140ud.dll", "mfcore.dll", "mfcsubs.dll", "mfds.dll", "mfdvdec.dll", "mferror.dll", "mfh263enc.dll", "mfh264enc.dll", "mfksproxy.dll", "MFMediaEngine.dll", "mfmjpegdec.dll", "mfmkvsrcsnk.dll", "mfmp4srcsnk.dll", "mfmpeg2srcsnk.dll", "mfnetcore.dll", "mfnetsrc.dll", "mfperfhelper.dll", "mfplat.dll", "MFPlay.dll", "mfps.dll", "mfreadwrite.dll", "mfsensorgroup.dll", "mfsrcsnk.dll", "mfsvr.dll", "mftranscode.dll", "mfvdsp.dll", "mfvfw.dll", "mfxplugin64_hw.dll", "mgmtapi.dll", "mgmtrefreshcredprov.dll", "mi.dll", "mibincodec.dll", "Microsoft-Windows-AppModelExecEvents.dll", "microsoft-windows-battery-events.dll", "microsoft-windows-hal-events.dll", "Microsoft-Windows-Internal-Shell-NearShareExperience.dll", "microsoft-windows-kernel-cc-events.dll", "microsoft-windows-kernel-pnp-events.dll", "microsoft-windows-kernel-power-events.dll", "microsoft-windows-kernel-processor-power-events.dll", "Microsoft-Windows-MapControls.dll", "Microsoft-Windows-MosHost.dll", "microsoft-windows-pdc.dll", "microsoft-windows-power-cad-events.dll", "microsoft-windows-processor-aggregator-events.dll", "microsoft-windows-sleepstudy-events.dll", "microsoft-windows-storage-tiering-events.dll", "microsoft-windows-system-events.dll", "Microsoft-WindowsPhone-SEManagementProvider.dll", "Microsoft.Bluetooth.Audio.dll", "Microsoft.Bluetooth.Proxy.dll", "Microsoft.Bluetooth.Service.dll", "Microsoft.Bluetooth.UserService.dll", "Microsoft.Graphics.Display.DisplayEnhancementService.dll", "Microsoft.Internal.FrameworkUdk.System.dll", "Microsoft.LocalUserImageProvider.dll", "Microsoft.Management.Infrastructure.Native.Unmanaged.dll", "Microsoft.Windows.Storage.Core.dll", "Microsoft.Windows.Storage.StorageBusCache.dll", "MicrosoftAccount.TokenProvider.Core.dll", "MicrosoftAccountCloudAP.dll", "MicrosoftAccountExtension.dll", "MicrosoftAccountTokenProvider.dll", "MicrosoftAccountWAMExtension.dll", "midimap.dll", "migisol.dll", "miguiresource.dll", "mimefilt.dll", "mimofcodec.dll", "MinstoreEvents.dll", "MiracastInputMgr.dll", "MiracastReceiver.dll", "MiracastReceiverExt.dll", "MirrorDrvCompat.dll", "mispace.dll", "MitigationClient.dll", "MitigationConfiguration.dll", "miutils.dll", "MixedReality.Broker.dll", "MixedRealityCapture.Pipeline.dll", "MixedRealityCapture.ProxyStub.dll", "MixedRealityRuntime.dll", "mlang.dll", "mmcbase.dll", "mmcndmgr.dll", "mmcshext.dll", "MMDevAPI.dll", "mmgaclient.dll", "mmgaproxystub.dll", "mmres.dll", "mobilenetworking.dll", "modemui.dll", "modernexecserver.dll", "moricons.dll", "moshost.dll", "MosHostClient.dll", "moshostcore.dll", "MosStorage.dll", "mpeval.dll", "mpr.dll", "mprapi.dll", "mprddm.dll", "mprdim.dll", "mprext.dll", "mprmsg.dll", "MPSSVC.dll", "mpunits.dll", "MrmCoreR.dll", "MrmDeploy.dll", "MrmIndexer.dll", "mrt100.dll", "mrt_map.dll", "ms3dthumbnailprovider.dll", "msaatext.dll", "msacm32.dll", "msafd.dll", "MSAJApi.dll", "MSAlacDecoder.dll", "MSAlacEncoder.dll", "MSAMRNBDecoder.dll", "MSAMRNBEncoder.dll", "MSAMRNBSink.dll", "MSAMRNBSource.dll", "MsApoFxProxy.dll", "MSAProfileNotificationHandler.dll", "msasn1.dll", "MSAudDecMFT.dll", "msaudite.dll", "msauserext.dll", "mscandui.dll", "mscat32.dll", "msclmd.dll", "mscms.dll", "mscoree.dll", "mscorier.dll", "mscories.dll", "msctf.dll", "MsCtfMonitor.dll", "msctfp.dll", "msctfui.dll", "msctfuimanager.dll", "msdadiag.dll", "msdart.dll", "msdelta.dll", "msdmo.dll", "msdrm.dll", "msdtckrm.dll", "msdtclog.dll", "msdtcprx.dll", "msdtcspoffln.dll", "msdtctm.dll", "msdtcuiu.dll", "msdtcVSp1res.dll", "msfeeds.dll", "msfeedsbs.dll", "MSFlacDecoder.dll", "MSFlacEncoder.dll", "msftedit.dll", "MsftOemDllIgneous.dll", "MSHEIF.dll", "mshtml.dll", "MshtmlDac.dll", "mshtmled.dll", "mshtmler.dll", "msi.dll", "MsiCofire.dll", "msidcrl40.dll", "msident.dll", "msidle.dll", "msidntld.dll", "msieftp.dll", "msihnd.dll", "msiltcfg.dll", "msimg32.dll", "msimsg.dll", "msimtf.dll", "msisip.dll", "msIso.dll", "msiwer.dll", "MsixDataSourceExtensionPS.dll", "mskeyprotcli.dll", "mskeyprotect.dll", "msls31.dll", "msmpeg2adec.dll", "msmpeg2vdec.dll", "msobjs.dll", "msodbcdiag11.dll", "msodbcdiag17.dll", "msodbcsql11.dll", "msodbcsql17.dll", "msoert2.dll", "msoledbsql.dll", "MSOpusDecoder.dll", "mspatcha.dll", "mspatchc.dll", "MSPhotography.dll", "msports.dll", "msprivs.dll", "msrahc.dll", "msrating.dll", "MSRAWImage.dll", "msrdc.dll", "MsRdpWebAccess.dll", "msrle32.dll", "msscntrs.dll", "mssign32.dll", "mssip32.dll", "mssitlb.dll", "MsSpellCheckingFacility.dll", "mssph.dll", "mssprxy.dll", "mssrch.dll", "mssvp.dll", "mstask.dll", "msTextPrediction.dll", "mstscax.dll", "msutb.dll", "msv1_0.dll", "msvcirt.dll", "msvcp100.dll", "msvcp110.dll", "msvcp110_win.dll", "msvcp120.dll", "msvcp120_clr0400.dll", "msvcp140.dll", "msvcp140d.dll", "msvcp140d_atomic_wait.dll", "msvcp140d_codecvt_ids.dll", "msvcp140_1.dll", "msvcp140_1d.dll", "msvcp140_2.dll", "msvcp140_2d.dll", "msvcp140_atomic_wait.dll", "msvcp140_clr0400.dll", "msvcp140_codecvt_ids.dll", "msvcp60.dll", "msvcp_win.dll", "msvcr100.dll", "msvcr100_clr0400.dll", "msvcr110.dll", "msvcr120.dll", "msvcr120_clr0400.dll", "msvcrt.dll", "msvfw32.dll", "msvidc32.dll", "MSVidCtl.dll", "MSVideoDSP.dll", "MSVP9DEC.dll", "msvproc.dll", "MSVPXENC.dll", "MSWB7.dll", "MSWB70011.dll", "MSWB70804.dll", "MSWebp.dll", "mswmdm.dll", "mswsock.dll", "msxml3.dll", "msxml3r.dll", "msxml6.dll", "msxml6r.dll", "msyuv.dll", "MtcModel.dll", "MTF.dll", "MTFAppServiceDS.dll", "MtfDecoder.dll", "MTFFuzzyDS.dll", "MTFServer.dll", "MTFSpellcheckDS.dll", "mtxclu.dll", "mtxdm.dll", "mtxex.dll", "mtxoci.dll", "muifontsetup.dll", "MUILanguageCleanup.dll", "museuxdocked.dll", "MusUpdateHandlers.dll", "mycomput.dll", "mydocs.dll", "NahimicAPO3ConfiguratorDaemonModule.dll", "NahimicAPO4.dll", "NahimicAPO4API.dll", "NahimicAPO4ConfiguratorDaemonModule.dll", "NahimicAPO4ExpertAPI.dll", "NahimicPnPAPO4ConfiguratorDaemonModule.dll", "NapiNSP.dll", "NaturalAuth.dll", "NaturalAuthClient.dll", "NaturalLanguage6.dll", "navshutdown.dll", "NcaApi.dll", "NcaSvc.dll", "ncbservice.dll", "NcdAutoSetup.dll", "NcdProp.dll", "nci.dll", "ncobjapi.dll", "ncrypt.dll", "ncryptprov.dll", "ncryptsslp.dll", "ncsi.dll", "ncuprov.dll", "nddeapi.dll", "ndfapi.dll", "ndfetw.dll", "ndfhcdiscovery.dll", "ndishc.dll", "ndproxystub.dll", "nduprov.dll", "negoexts.dll", "netapi32.dll", "netbios.dll", "netcenter.dll", "netcfgx.dll", "netcorehc.dll", "netdiagfx.dll", "NetDriverInstall.dll", "netevent.dll", "netfxperf.dll", "neth.dll", "netid.dll", "netiohlp.dll", "netjoin.dll", "netlogon.dll", "netman.dll", "NetMgmtIF.dll", "netmsg.dll", "netplwiz.dll", "netprofm.dll", "netprofmsvc.dll", "netprovfw.dll", "netprovisionsp.dll", "NetSetupApi.dll", "NetSetupEngine.dll", "NetSetupShim.dll", "NetSetupSvc.dll", "netshell.dll", "nettrace.dll", "netutils.dll", "NetworkBindingEngineMigPlugin.dll", "NetworkCollectionAgent.dll", "NetworkDesktopSettings.dll", "networkexplorer.dll", "networkhelper.dll", "NetworkIcon.dll", "networkitemfactory.dll", "NetworkMobileSettings.dll", "NetworkProxyCsp.dll", "NetworkQoSPolicyCSP.dll", "NetworkUXBroker.dll", "newdev.dll", "NFCProvisioningPlugin.dll", "NfcRadioMedia.dll", "ngccredprov.dll", "NgcCtnr.dll", "NgcCtnrGidsHandler.dll", "NgcCtnrSvc.dll", "NgcIsoCtnr.dll", "ngckeyenum.dll", "ngcksp.dll", "ngclocal.dll", "ngcpopkeysrv.dll", "NgcProCsp.dll", "ngcrecovery.dll", "ngcsvc.dll", "ngctasks.dll", "ngcutils.dll", "ngraph.dll", "NhNotifSys.dll", "ninput.dll", "NL7Data0011.dll", "NL7Data0804.dll", "NL7Lexicons0011.dll", "NL7Lexicons0804.dll", "NL7Models0011.dll", "NL7Models0804.dll", "nlaapi.dll", "nlahc.dll", "nlansp_c.dll", "nlhtml.dll", "nlmgp.dll", "nlmproxy.dll", "nlmsprep.dll", "nlsbres.dll", "NlsData0000.dll", "NlsData0009.dll", "Nlsdl.dll", "NlsLexicons0009.dll", "NmaDirect.dll", "noise.dll", "nonarpinv.dll", "normaliz.dll", "NotificationController.dll", "NotificationControllerPS.dll", "NotificationIntelligencePlatform.dll", "notificationplatformcomponent.dll", "npmproxy.dll", "NPSM.dll", "NPSMDesktopProvider.dll", "nrpsrv.dll", "nrtapi.dll", "nshhttp.dll", "nshipsec.dll", "nshwfp.dll", "nsi.dll", "nsisvc.dll", "ntasn1.dll", "ntdll.dll", "ntdsapi.dll", "ntfsres.dll", "ntlanman.dll", "ntlanui2.dll", "NtlmShared.dll", "ntmarta.dll", "ntprint.dll", "ntshrui.dll", "ntvdm64.dll", "NvAgent.dll", "nvapi64.dll", "nvaudcap64v.dll", "nvcpl.dll", "nvcuda.dll", "nvcudadebugger.dll", "nvcuvid.dll", "nvEncodeAPI64.dll", "NvFBC64.dll", "NvIFR64.dll", "nvml.dll", "nvofapi64.dll", "NvRtmpStreamer64.dll", "nvspcap64.dll", "objsel.dll", "occache.dll", "ocsetapi.dll", "odbc32.dll", "odbcbcp.dll", "odbcconf.dll", "odbccp32.dll", "odbccr32.dll", "odbccu32.dll", "odbcint.dll", "odbctrac.dll", "OEMDefaultAssociations.dll", "oemlicense.dll", "offfilt.dll", "officecsp.dll", "offlinelsa.dll", "offlinesam.dll", "offreg.dll", "ole32.dll", "oleacc.dll", "oleacchooks.dll", "oleaccrc.dll", "oleaut32.dll", "oledlg.dll", "oleprn.dll", "OmaDmAgent.dll", "omadmapi.dll", "OnDemandBrokerClient.dll", "OnDemandConnRouteHelper.dll", "OneBackupHandler.dll", "OneCoreCommonProxyStub.dll", "OneCoreUAPCommonProxyStub.dll", "OneSettingsClient.dll", "onex.dll", "onexui.dll", "onnxruntime.dll", "OpcServices.dll", "OpenCL.dll", "opengl32.dll", "ortcengine.dll", "osbaseln.dll", "OskSupport.dll", "osuninst.dll", "P2P.dll", "P2PGraph.dll", "p2pnetsh.dll", "p2psvc.dll", "p9np.dll", "p9rdrservice.dll", "packager.dll", "PackageStateChangeHandler.dll", "panmap.dll", "PasswordEnrollmentManager.dll", "pautoenr.dll", "PayloadRestrictions.dll", "PaymentMediatorServiceProxy.dll", "pcacli.dll", "pcadm.dll", "pcaevts.dll", "pcasvc.dll", "pcaui.dll", "PCPKsp.dll", "PCShellCommonProxyStub.dll", "pcsvDevice.dll", "pcwum.dll", "pcwutl.dll", "pdh.dll", "pdhui.dll", "PenService.dll", "PeopleAPIs.dll", "PeopleBand.dll", "PerceptionDevice.dll", "PerceptionSimulation.ProxyStubs.dll", "PerceptionSimulationManager.dll", "perf-MSSQL$SQLEXPRESS-sqlctr15.0.2000.5.dll", "perf-MSSQL15.SQLEXPRESS-sqlagtctr.dll", "perfdisk.dll", "perfnet.dll", "perfos.dll", "perfproc.dll", "perfts.dll", "perf_gputiming.dll", "PersonalizationCSP.dll", "pfclient.dll", "PhoneCallHistoryApis.dll", "PhoneOm.dll", "PhonePlatformAbstraction.dll", "PhoneProviders.dll", "PhoneService.dll", "PhoneServiceRes.dll", "Phoneutil.dll", "PhoneutilRes.dll", "PhotoMetadataHandler.dll", "photowiz.dll", "PickerPlatform.dll", "pid.dll", "pidgenx.dll", "pifmgr.dll", "PimIndexMaintenance.dll", "PimIndexMaintenanceClient.dll", "Pimstore.dll", "PinEnrollmentHelper.dll", "pkeyhelper.dll", "PktMonApi.dll", "pku2u.dll", "pla.dll", "playlistfolder.dll", "PlaySndSrv.dll", "PlayToDevice.dll", "PlayToManager.dll", "playtomenu.dll", "PlayToReceiver.dll", "PlayToStatusProvider.dll", "ploptin.dll", "pngfilt.dll", "pnidui.dll", "pnpclean.dll", "pnpdiag.dll", "pnppolicy.dll", "pnpts.dll", "pnpui.dll", "PNPXAssoc.dll", "PNPXAssocPrx.dll", "pnrpauto.dll", "Pnrphc.dll", "pnrpnsp.dll", "pnrpsvc.dll", "policymanager.dll", "policymanagerprecheck.dll", "polstore.dll", "PortableDeviceApi.dll", "PortableDeviceClassExtension.dll", "PortableDeviceConnectApi.dll", "PortableDeviceStatus.dll", "PortableDeviceSyncProvider.dll", "PortableDeviceTypes.dll", "PortableDeviceWiaCompat.dll", "posetup.dll", "POSyncServices.dll", "pots.dll", "powercpl.dll", "powrprof.dll", "prauthproviders.dll", "PresentationCFFRasterizerNative_v0300.dll", "PresentationHostProxy.dll", "PresentationNative_v0300.dll", "prflbmsg.dll", "Print.PrintSupport.Source.dll", "Print.Workflow.Source.dll", "PrinterCleanupTask.dll", "printfilterpipelineprxy.dll", "PrintIsolationProxy.dll", "PrintNotification.dll", "PrintPlatformConfig.dll", "printticketvalidation.dll", "printui.dll", "PrintWorkflowService.dll", "PrintWSDAHost.dll", "prm0009.dll", "prm0019.dll", "prncache.dll", "prnfldr.dll", "prnntfy.dll", "prntvpt.dll", "ProductEnumerator.dll", "profapi.dll", "profext.dll", "profprov.dll", "profsvc.dll", "profsvcext.dll", "propsys.dll", "provcore.dll", "provdatastore.dll", "provdiagnostics.dll", "provengine.dll", "provhandlers.dll", "provisioningcommandscsp.dll", "provisioningcsp.dll", "ProvisioningHandlers.dll", "provmigrate.dll", "provops.dll", "provpackageapidll.dll", "provplatformdesktop.dll", "ProvPluginEng.dll", "ProvSysprep.dll", "provthrd.dll", "ProximityCommon.dll", "ProximityCommonPal.dll", "ProximityRtapiPal.dll", "ProximityService.dll", "ProximityServicePal.dll", "prvdmofcomp.dll", "prxyqry.dll", "psapi.dll", "psisdecd.dll", "PSModuleDiscoveryProvider.dll", "PsmServiceExtHost.dll", "psmsrv.dll", "pstask.dll", "pstorec.dll", "ptpprov.dll", "puiapi.dll", "puiobj.dll", "PushToInstall.dll", "PwdlessAggregator.dll", "pwlauncher.dll", "pwrshplugin.dll", "pwrshsip.dll", "pwsso.dll", "qasf.dll", "qcap.dll", "qdv.dll", "qdvd.dll", "qedit.dll", "qedwipes.dll", "qmgr.dll", "QualityUpdateAssistant.dll", "quartz.dll", "Query.dll", "QuickActionsDataModel.dll", "QuietHours.dll", "qwave.dll", "RacEngn.dll", "racpldlg.dll", "radardt.dll", "radarrs.dll", "RADCUI.dll", "RandomAccessStreamDataSource.dll", "rasadhlp.dll", "rasapi32.dll", "rasauto.dll", "raschap.dll", "raschapext.dll", "rasctrs.dll", "rascustom.dll", "rasdiag.dll", "rasdlg.dll", "rasgcw.dll", "rasman.dll", "rasmans.dll", "rasmbmgr.dll", "RasMediaManager.dll", "RASMM.dll", "rasmontr.dll", "rasplap.dll", "rasppp.dll", "rastapi.dll", "rastls.dll", "rastlsext.dll", "rdbui.dll", "rdp4vs.dll", "RdpAvenc.dll", "rdpbase.dll", "rdpcfgex.dll", "rdpcorets.dll", "rdpcredentialprovider.dll", "rdpendp.dll", "rdpnanoTransport.dll", "RdpRelayTransport.dll", "RdpSaPs.dll", "rdpserverbase.dll", "rdpsharercom.dll", "rdpudd.dll", "rdpviewerax.dll", "RDSAppXHelper.dll", "rdsdwmdr.dll", "rdvvmtransport.dll", "RDXService.dll", "RDXTaskFactory.dll", "ReAgent.dll", "ReAgentTask.dll", "recovery.dll", "regapi.dll", "RegCtrl.dll", "regidle.dll", "regsvc.dll", "reguwpapi.dll", "ReInfo.dll", "remoteaudioendpoint.dll", "remotepg.dll", "RemoteWipeCSP.dll", "RemovableMediaProvisioningPlugin.dll", "RemoveDeviceContextHandler.dll", "RemoveDeviceElevated.dll", "ReportingCSP.dll", "ResBParser.dll", "reseteng.dll", "ResetEngine.dll", "ResetEngOnline.dll", "ResourceMapper.dll", "ResourcePolicyClient.dll", "ResourcePolicyServer.dll", "resutils.dll", "rgb9rast.dll", "riched20.dll", "riched32.dll", "RjvMDMConfig.dll", "RMapi.dll", "rmclient.dll", "RMSRoamingSecurity.dll", "rnr20.dll", "RoamingSecurity.dll", "rometadata.dll", "RotMgr.dll", "RpcEpMap.dll", "rpchttp.dll", "RpcNs4.dll", "rpcnsh.dll", "rpcrt4.dll", "RpcRtRemote.dll", "rpcss.dll", "rsaenh.dll", "rshx32.dll", "RstrtMgr.dll", "rtffilt.dll", "rtm.dll", "rtmcodecs.dll", "RTMediaFrame.dll", "rtmmvrortc.dll", "rtmpal.dll", "rtmpltfm.dll", "rtpm.dll", "rtutils.dll", "RTWorkQ.dll", "RuleBasedDS.dll", "rundll32.exe", "samcli.dll", "samlib.dll", "samsrv.dll", "sas.dll", "sbe.dll", "sbeio.dll", "sberes.dll", "sbresources.dll", "sbservicetrigger.dll", "scansetting.dll", "SCardBi.dll", "SCardDlg.dll", "SCardSvr.dll", "scavengeui.dll", "ScDeviceEnum.dll", "scecli.dll", "scesrv.dll", "schannel.dll", "schedcli.dll", "schedsvc.dll", "scksp.dll", "scripto.dll", "scrobj.dll", "scrptadm.dll", "scrrun.dll", "sdcpl.dll", "SDDS.dll", "sdengin2.dll", "SDFHost.dll", "sdhcinst.dll", "sdiageng.dll", "sdiagprv.dll", "sdiagschd.dll", "sdohlp.dll", "sdrsvc.dll", "sdshext.dll", "Search.ProtocolHandler.MAPI2.dll", "SearchFolder.dll", "SearchIndexerCore.dll", "SebBackgroundManagerPolicy.dll", "SecEditCtl.BCM.x64.dll", "secfw_AuthenticAMD.dll", "sechost.dll", "seclogon.dll", "secproc.dll", "secproc_isv.dll", "secproc_ssp.dll", "secproc_ssp_isv.dll", "secur32.dll", "SecureTimeAggregator.dll", "security.dll", "SecurityCenterBroker.dll", "SecurityCenterBrokerPS.dll", "SecurityHealthAgent.dll", "SecurityHealthCore.dll", "SecurityHealthProxyStub.dll", "SecurityHealthSSO.dll", "SecurityHealthSsoUdk.dll", "SecurityHealthUdk.dll", "sedplugins.dll", "SEMgrPS.dll", "SEMgrSvc.dll", "sendmail.dll", "Sens.dll", "SensApi.dll", "SensorPerformanceEvents.dll", "SensorsApi.dll", "SensorsClassExtension.dll", "SensorsCpl.dll", "SensorService.dll", "SensorsNativeApi.dll", "SensorsNativeApi.V2.dll", "SensorsUtilsV2.dll", "sensrsvc.dll", "serialui.dll", "ServicingCommon.dll", "ServicingUAPI.dll", "serwvdrv.dll", "SessEnv.dll", "setbcdlocale.dll", "SetNetworkLocation.dll", "SetNetworkLocationFlyout.dll", "SetProxyCredential.dll", "SettingsEnvironment.Desktop.dll", "SettingsExtensibilityHandlers.dll", "SettingsHandlers_Accessibility.dll", "SettingsHandlers_AdvertisingId.dll", "SettingsHandlers_AnalogShell.dll", "SettingsHandlers_AppControl.dll", "SettingsHandlers_AppExecutionAlias.dll", "SettingsHandlers_Authentication.dll", "SettingsHandlers_BackgroundApps.dll", "SettingsHandlers_Backup.dll", "SettingsHandlers_BatteryUsage.dll", "SettingsHandlers_Camera.dll", "SettingsHandlers_CapabilityAccess.dll", "SettingsHandlers_Clipboard.dll", "SettingsHandlers_ClosedCaptioning.dll", "SettingsHandlers_CloudPC.dll", "SettingsHandlers_ContentDeliveryManager.dll", "SettingsHandlers_Cortana.dll", "SettingsHandlers_DesktopTaskbar.dll", "SettingsHandlers_Devices.dll", "SettingsHandlers_Display.dll", "SettingsHandlers_Flights.dll", "SettingsHandlers_Fonts.dll", "SettingsHandlers_ForceSync.dll", "SettingsHandlers_Gaming.dll", "SettingsHandlers_Geolocation.dll", "SettingsHandlers_Gpu.dll", "SettingsHandlers_HoloLens_Environment.dll", "SettingsHandlers_HumanPresence.dll", "SettingsHandlers_IME.dll", "SettingsHandlers_InkingTypingPrivacy.dll", "SettingsHandlers_InputPersonalization.dll", "SettingsHandlers_InstalledUpdates.dll", "SettingsHandlers_Keyboard.dll", "SettingsHandlers_Language.dll", "SettingsHandlers_Lighting.dll", "SettingsHandlers_ManagePhone.dll", "SettingsHandlers_Maps.dll", "SettingsHandlers_Mouse.dll", "SettingsHandlers_Notifications.dll", "SettingsHandlers_nt.dll", "SettingsHandlers_OneCore_BatterySaver.dll", "SettingsHandlers_OneCore_PowerAndSleep.dll", "SettingsHandlers_OneDriveBackup.dll", "SettingsHandlers_OptionalFeatures.dll", "SettingsHandlers_PCDisplay.dll", "SettingsHandlers_Pen.dll", "SettingsHandlers_Region.dll", "SettingsHandlers_SharedExperiences_Rome.dll", "SettingsHandlers_SIUF.dll", "SettingsHandlers_SpeechPrivacy.dll", "SettingsHandlers_Startup.dll", "SettingsHandlers_Storage.dll", "SettingsHandlers_StorageSense.dll", "SettingsHandlers_Touch.dll", "SettingsHandlers_Troubleshoot.dll", "SettingsHandlers_User.dll", "SettingsHandlers_UserAccount.dll", "SettingsHandlers_UserExperience.dll", "SettingsHandlers_UserIntent.dll", "SettingsHandlers_WorkAccess.dll", "SettingSyncDownloadHelper.dll", "setupapi.dll", "setupcl.dll", "setupcln.dll", "setupetw.dll", "SFAPE.dll", "SFAPM.dll", "sfc.dll", "sfc_os.dll", "sgl_mnn_dll.dll", "shacct.dll", "shacctprofile.dll", "SharedPCCSP.dll", "SharedRealitySvc.dll", "ShareHost.dll", "sharemediacpl.dll", "SHCore.dll", "shdocvw.dll", "shell32.dll", "ShellCommonCommonProxyStub.dll", "shellstyle.dll", "shfolder.dll", "shgina.dll", "shimeng.dll", "shimgvw.dll", "shlwapi.dll", "shpafact.dll", "shsetup.dll", "shsvcs.dll", "shunimpl.dll", "shutdownext.dll", "shutdownux.dll", "shwebsvc.dll", "signdrv.dll", "SimAuth.dll", "SimCfg.dll", "skci.dll", "slc.dll", "slcext.dll", "slwga.dll", "SmartActionPlatform.dll", "SmartCardBackgroundPolicy.dll", "SmartcardCredentialProvider.dll", "SmartCardSimulator.dll", "smartscreen.dll", "smartscreenps.dll", "SmartWorkflows.dll", "SMBHelperClass.dll", "smbwmiv2.dll", "SmiEngine.dll", "smphost.dll", "SmsRouterSvc.dll", "SndVolSSO.dll", "snmpapi.dll", "socialapis.dll", "softkbd.dll", "softpub.dll", "SortServer2003Compat.dll", "SortWindows61.dll", "SortWindows62.dll", "SortWindows63.dll", "SortWindows6Compat.dll", "SpaceControl.dll", "spatialinteraction.dll", "SpatializerApo.dll", "SpatialStore.dll", "spbcd.dll", "SpectrumSyncClient.dll", "spfileq.dll", "spinf.dll", "SPITDevMft64.dll", "spmpm.dll", "spnet.dll", "spoolss.dll", "spopk.dll", "spp.dll", "sppc.dll", "sppcext.dll", "sppcomapi.dll", "sppcommdlg.dll", "sppnp.dll", "sppobjs.dll", "sppwinob.dll", "sppwmi.dll", "spwinsat.dll", "spwizeng.dll", "spwizimg.dll", "spwizres.dll", "spwmp.dll", "sqlncli11.dll", "SqlServerSpatial120.dll", "SqlServerSpatial150.dll", "sqlsrv32.dll", "sqmapi.dll", "srchadmin.dll", "srclient.dll", "srcore.dll", "SrEvents.dll", "SRH.dll", "srhelper.dll", "srpapi.dll", "SrpUxNativeSnapIn.dll", "srrstr.dll", "srumapi.dll", "srumsvc.dll", "srvcli.dll", "srvsvc.dll", "srwmi.dll", "sscore.dll", "sscoreext.dll", "ssdm.dll", "ssdpapi.dll", "ssdpsrv.dll", "sspicli.dll", "sspisrv.dll", "SSShim.dll", "sstpcfg.dll", "sstpsvc.dll", "StartTileData.dll", "Startupscan.dll", "StateRepository.Core.dll", "stclient.dll", "sti.dll", "sti_ci.dll", "stobject.dll", "StorageContextHandler.dll", "StorageUsage.dll", "storagewmi.dll", "storagewmi_passthru.dll", "storewuauth.dll", "Storprop.dll", "StorSvc.dll", "streamci.dll", "StringFeedbackEngine.dll", "StructuredQuery.dll", "sud.dll", "SustainabilityService.dll", "svf.dll", "svsvc.dll", "SwitcherDataModel.dll", "swprv.dll", "sxproxy.dll", "sxs.dll", "sxshared.dll", "sxssrv.dll", "sxsstore.dll", "SyncCenter.dll", "SyncController.dll", "SyncHostps.dll", "SyncInfrastructure.dll", "SyncInfrastructureps.dll", "SyncProxy.dll", "Syncreg.dll", "SyncRes.dll", "SyncSettings.dll", "syncutil.dll", "sysclass.dll", "SysFxUI.dll", "sysmain.dll", "sysntfy.dll", "syssetup.dll", "systemcpl.dll", "SystemEventsBrokerClient.dll", "SystemEventsBrokerServer.dll", "SystemSettings.DataModel.dll", "SystemSettings.DeviceEncryptionHandlers.dll", "SystemSettings.Handlers.dll", "SystemSettings.SettingsExtensibility.dll", "SystemSettings.UserAccountsHandlers.dll", "SystemSettingsThresholdAdminFlowUI.dll", "SystemSupportInfo.dll", "t2embed.dll", "Tabbtn.dll", "TabbtnEx.dll", "TabSvc.dll", "tapi3.dll", "tapi32.dll", "tapilua.dll", "TapiMigPlugin.dll", "tapiperf.dll", "tapisrv.dll", "TapiSysprep.dll", "tapiui.dll", "TaskApis.dll", "Taskbar.dll", "taskbarcpl.dll", "taskcomp.dll", "TaskFlowDataEngine.dll", "TaskManagerDataLayer.dll", "taskschd.dll", "TaskSchdPS.dll", "tbauth.dll", "tbb.dll", "tbs.dll", "tcbloader.dll", "tcpipcfg.dll", "tcpmib.dll", "tcpmon.dll", "tcpmonui.dll", "tdh.dll", "tdhres.dll", "TDLMigration.dll", "TEEManagement64.dll", "TelephonyInteractiveUser.dll", "TelephonyInteractiveUserRes.dll", "TempSignedLicenseExchangeTask.dll", "termmgr.dll", "termsrv.dll", "tetheringclient.dll", "tetheringconfigsp.dll", "TetheringIeProvider.dll", "TetheringMgr.dll", "tetheringservice.dll", "TetheringStation.dll", "TextInputFramework.dll", "TextInputMethodFormatter.dll", "TextShaping.dll", "themecpl.dll", "Themes.SsfDownload.ScheduledTask.dll", "themeservice.dll", "themeui.dll", "threadpoolwinrt.dll", "ThreatAssessment.dll", "ThreatExperienceManager.dll", "ThreatIntelligence.dll", "ThreatResponseEngine.dll", "thumbcache.dll", "tier2punctuations.dll", "TieringEngineProxy.dll", "TileDataRepository.dll", "TimeBrokerClient.dll", "TimeBrokerServer.dll", "TimeDateMUICallback.dll", "timesync.dll", "TimeSyncTask.dll", "tlscsp.dll", "tokenbinding.dll", "TokenBroker.dll", "TokenBrokerUI.dll", "TpmCertResources.dll", "tpmcompc.dll", "TpmCoreProvisioning.dll", "TpmEngUM.dll", "TpmEngUM138.dll", "TpmTasks.dll", "tpmvsc.dll", "tprtdll.dll", "tquery.dll", "traffic.dll", "TransliterationRanker.dll", "trie.dll", "trkwks.dll", "TrustedSignalCredProv.dll", "tsbyuv.dll", "tsf3gip.dll", "tsgqec.dll", "tsmf.dll", "TSpkg.dll", "TSSessionUX.dll", "TsUsbGDCoInstaller.dll", "TsUsbRedirectionGroupPolicyExtension.dll", "TSWorkspace.dll", "ttdloader.dll", "ttdplm.dll", "ttdrecord.dll", "ttdrecordcpu.dll", "TtlsAuth.dll", "TtlsCfg.dll", "TtlsExt.dll", "tvratings.dll", "twext.dll", "twinapi.appcore.dll", "twinapi.dll", "twinui.appcore.dll", "twinui.dll", "twinui.pcshell.dll", "txflog.dll", "txfw32.dll", "tzautoupdate.dll", "tzres.dll", "tzsyncres.dll", "ubpm.dll", "ucmhc.dll", "ucrtbase.dll", "ucrtbased.dll", "ucrtbase_clr0400.dll", "ucrtbase_enclave.dll", "udhisapi.dll", "uDWM.dll", "UefiCsp.dll", "uexfat.dll", "ufat.dll", "UiaManager.dll", "UIAnimation.dll", "UIAutomationCore.dll", "uicom.dll", "UIManagerBrokerps.dll", "uireng.dll", "UIRibbon.dll", "UIRibbonRes.dll", "ulib.dll", "umb.dll", "umdmxfrm.dll", "umpdc.dll", "umpnpmgr.dll", "umpo-overrides.dll", "umpo.dll", "umpodev.dll", "umpoext.dll", "umpowmi.dll", "umrdp.dll", "unattend.dll", "unenrollhook.dll", "UnifiedConsent.dll", "unimdmat.dll", "uniplat.dll", "Unistore.dll", "untfs.dll", "UpdateAgent.dll", "updatecsp.dll", "UpdateHeartbeatScan.dll", "updatepolicy.dll", "UpdatePolicyScenarioReliabilityAggregator.dll", "UpdateReboot.dll", "upnp.dll", "upnphost.dll", "UPPrinterInstallsCSP.dll", "upshared.dll", "uReFS.dll", "uReFSv1.dll", "ureg.dll", "url.dll", "urlmon.dll", "UsbCApi.dll", "usbceip.dll", "usbmon.dll", "usbperf.dll", "UsbPmApi.dll", "UsbSettingsHandlers.dll", "UsbTask.dll", "usbui.dll", "user32.dll", "UserAccountControlSettings.dll", "useractivitybroker.dll", "usercpl.dll", "UserDataAccessRes.dll", "UserDataAccountApis.dll", "UserDataLanguageUtil.dll", "UserDataPlatformHelperUtil.dll", "UserDataService.dll", "UserDataTimeUtil.dll", "UserDataTypeHelperUtil.dll", "UserDeviceRegistration.dll", "UserDeviceRegistration.Ngc.dll", "userenv.dll", "userinitext.dll", "UserLanguageProfileCallback.dll", "usermgr.dll", "usermgrcli.dll", "UserMgrProxy.dll", "usoapi.dll", "usocoreps.dll", "usodocked.dll", "usosvc.dll", "usosvcimpl.dll", "usp10.dll", "ustprov.dll", "utcapi.dll", "utcutil.dll", "utildll.dll", "uudf.dll", "UvcModel.dll", "UXInit.dll", "uxlib.dll", "uxlibres.dll", "uxtheme.dll", "vac.dll", "VAN.dll", "Vault.dll", "VaultCDS.dll", "vaultcli.dll", "VaultRoaming.dll", "vaultsvc.dll", "vbsapi.dll", "vbscript.dll", "vbssysprep.dll", "vcamp110.dll", "vcamp120.dll", "vcamp140.dll", "vcamp140d.dll", "VCardParser.dll", "vccorlib110.dll", "vccorlib120.dll", "vccorlib140.dll", "vccorlib140d.dll", "vcomp100.dll", "vcomp110.dll", "vcomp120.dll", "vcomp140.dll", "vcomp140d.dll", "vcruntime140.dll", "vcruntime140d.dll", "vcruntime140_1.dll", "vcruntime140_1d.dll", "vcruntime140_1_clr0400.dll", "vcruntime140_clr0400.dll", "vcruntime140_threads.dll", "vcruntime140_threadsd.dll", "vdsbas.dll", "vdsdyn.dll", "vdsutil.dll", "vdsvd.dll", "vds_ps.dll", "verifier.dll", "version.dll", "vertdll.dll", "vfbasics.dll", "vfcompat.dll", "vfcuzz.dll", "vfluapriv.dll", "vfnet.dll", "vfntlmless.dll", "vfnws.dll", "vfpapi.dll", "vfprint.dll", "vfprintpthelper.dll", "vfrdvcompat.dll", "vfuprov.dll", "vfwwdm32.dll", "VhfUm.dll", "vid.dll", "VideoHandlers.dll", "virtdisk.dll", "VirtualMonitorManager.dll", "VirtualSurroundApo.dll", "VmApplicationHealthMonitorProxy.dll", "vmbuspipe.dll", "vmbuspiper.dll", "vmbusvdev.dll", "vmchipset.dll", "vmcompute.dll", "vmcomputeeventlog.dll", "VmCrashDump.dll", "VmDataStore.dll", "vmdebug.dll", "vmdevicehost.dll", "vmdynmem.dll", "vmemulateddevices.dll", "VmEmulatedNic.dll", "VmEmulatedStorage.dll", "vmfirmware.dll", "vmfirmwarehcl.dll", "vmfirmwarepcat.dll", "vmflexio.dll", "vmhbmgmt.dll", "vmhgs.dll", "vmiccore.dll", "vmicrdv.dll", "vmictimeprovider.dll", "vmicvdev.dll", "vmmsprox.dll", "vmpmem.dll", "vmprox.dll", "vmrdvcore.dll", "vmserial.dll", "vmsif.dll", "vmsifcore.dll", "vmsifproxystub.dll", "vmsmb.dll", "vmsynthfcvdev.dll", "VmSynthNic.dll", "vmsynthstor.dll", "vmtpm.dll", "vmuidevices.dll", "vmusrv.dll", "vmvirtio.dll", "vmvpci.dll", "vmwpctrl.dll", "vmwpevents.dll", "VocabRoamingHandler.dll", "VoiceActivationManager.dll", "VoipRT.dll", "vp9fs.dll", "vpcievdev.dll", "vpnike.dll", "vpnikeapi.dll", "VpnSohDesktop.dll", "VPNv2CSP.dll", "VrdUmed.dll", "vrfcore.dll", "VscMgrPS.dll", "vsconfig.dll", "vscover170.dll", "VSD3DWARPDebug.dll", "VsGraphicsCapture.dll", "VsGraphicsExperiment.dll", "VsGraphicsHelper.dll", "VsGraphicsProxyStub.dll", "VSPerf170.dll", "vssapi.dll", "vsstrace.dll", "vss_ps.dll", "vulkan-1-999-0-0-0.dll", "vulkan-1.dll", "w32time.dll", "w32topl.dll", "WaaSAssessment.dll", "WaaSMedicPS.dll", "WaaSMedicSvc.dll", "WABSyncProvider.dll", "WalletBackgroundServiceProxy.dll", "WalletProxy.dll", "WalletService.dll", "wamregps.dll", "wavemsp.dll", "wbemcomn.dll", "wbiosrvc.dll", "wci.dll", "wcimage.dll", "wcmapi.dll", "wcmcsp.dll", "wcmsvc.dll", "WcnApi.dll", "wcncsvc.dll", "WcnEapAuthProxy.dll", "WcnEapPeerProxy.dll", "WcnNetsh.dll", "wcnwiz.dll", "wc_storage.dll", "wdc.dll", "WdfCoInstaller01009.dll", "wdi.dll", "wdigest.dll", "wdscore.dll", "weasel.dll", "webauthn.dll", "WebcamUi.dll", "webcheck.dll", "WebClnt.dll", "webio.dll", "webplatstorageserver.dll", "WebRuntimeManager.dll", "webservices.dll", "Websocket.dll", "webthreatdefsvc.dll", "webthreatdefusersvc.dll", "wecapi.dll", "wecsvc.dll", "wephostsvc.dll", "wer.dll", "werconcpl.dll", "wercplsupport.dll", "werdiagcontroller.dll", "WerEnc.dll", "weretw.dll", "wersvc.dll", "werui.dll", "wevtapi.dll", "wevtfwd.dll", "wevtsvc.dll", "wfapigp.dll", "wfdprov.dll", "WFDSConMgr.dll", "WFDSConMgrSvc.dll", "WfHC.dll", "WFSR.dll", "whealogr.dll", "whhelper.dll", "wiaaut.dll", "wiadefui.dll", "wiadss.dll", "WiaExtensionHost64.dll", "wiarpc.dll", "wiascanprofiles.dll", "wiaservc.dll", "wiashext.dll", "wiatrace.dll", "WiFiCloudStore.dll", "WiFiConfigSP.dll", "wifidatacapabilityhandler.dll", "WiFiDisplay.dll", "wifinetworkmanager.dll", "wimgapi.dll", "win32appinventorycsp.dll", "Win32CompatibilityAppraiserCSP.dll", "win32spl.dll", "win32u.dll", "Win32_DeviceGuard.dll", "winbio.dll", "WinBioDataModel.dll", "winbioext.dll", "winbrand.dll", "wincorlib.dll", "wincredprovider.dll", "wincredui.dll", "windlp.dll", "WindowManagement.dll", "WindowManagementAPI.dll", "Windows.AccountsControl.dll", "Windows.AI.MachineLearning.dll", "Windows.AI.MachineLearning.Preview.dll", "Windows.ApplicationModel.Background.SystemEventsBroker.dll", "Windows.ApplicationModel.Background.TimeBroker.dll", "Windows.ApplicationModel.ConversationalAgent.dll", "windows.applicationmodel.conversationalagent.internal.proxystub.dll", "windows.applicationmodel.conversationalagent.proxystub.dll", "Windows.ApplicationModel.Core.dll", "windows.applicationmodel.datatransfer.dll", "Windows.ApplicationModel.dll", "Windows.ApplicationModel.LockScreen.dll", "Windows.ApplicationModel.Store.dll", "Windows.ApplicationModel.Store.Preview.DOSettings.dll", "Windows.ApplicationModel.Store.TestingFramework.dll", "Windows.ApplicationModel.Wallet.dll", "Windows.CloudStore.dll", "Windows.CloudStore.EarlyDownloader.dll", "Windows.CloudStore.Schema.DesktopShell.dll", "Windows.CloudStore.Schema.Shell.dll", "Windows.Cortana.Desktop.dll", "Windows.Cortana.OneCore.dll", "Windows.Cortana.ProxyStub.dll", "Windows.Data.Activities.dll", "Windows.Data.Pdf.dll", "Windows.Devices.AllJoyn.dll", "Windows.Devices.Background.dll", "Windows.Devices.Background.ps.dll", "Windows.Devices.Bluetooth.dll", "Windows.Devices.Custom.dll", "Windows.Devices.Custom.ps.dll", "Windows.Devices.Enumeration.dll", "Windows.Devices.Haptics.dll", "Windows.Devices.HumanInterfaceDevice.dll", "Windows.Devices.Lights.dll", "Windows.Devices.LowLevel.dll", "Windows.Devices.Midi.dll", "Windows.Devices.Perception.dll", "Windows.Devices.Picker.dll", "Windows.Devices.PointOfService.dll", "Windows.Devices.Portable.dll", "Windows.Devices.Printers.dll", "Windows.Devices.Printers.Extensions.dll", "Windows.Devices.Radios.dll", "Windows.Devices.Scanners.dll", "Windows.Devices.Sensors.dll", "Windows.Devices.SerialCommunication.dll", "Windows.Devices.SmartCards.dll", "Windows.Devices.SmartCards.Phone.dll", "Windows.Devices.Usb.dll", "Windows.Devices.WiFi.dll", "Windows.Devices.WiFiDirect.dll", "Windows.Energy.dll", "Windows.FileExplorer.Common.dll", "Windows.Gaming.Input.dll", "Windows.Gaming.Preview.dll", "Windows.Gaming.UI.GameBar.dll", "Windows.Gaming.XboxLive.Storage.dll", "Windows.Globalization.dll", "Windows.Globalization.Fontgroups.dll", "Windows.Globalization.PhoneNumberFormatting.dll", "Windows.Graphics.Display.BrightnessOverride.dll", "Windows.Graphics.Display.DisplayEnhancementOverride.dll", "Windows.Graphics.dll", "Windows.Graphics.Printing.3D.dll", "Windows.Graphics.Printing.dll", "Windows.Graphics.Printing.Workflow.dll", "Windows.Graphics.Printing.Workflow.Native.dll", "Windows.Help.Runtime.dll", "windows.immersiveshell.serviceprovider.dll", "Windows.Internal.AdaptiveCards.XamlCardRenderer.dll", "Windows.Internal.CapturePicker.Desktop.dll", "Windows.Internal.CapturePicker.dll", "Windows.Internal.Devices.Bluetooth.dll", "Windows.Internal.Devices.Sensors.dll", "Windows.Internal.Feedback.Analog.dll", "Windows.Internal.Feedback.Analog.ProxyStub.dll", "Windows.Internal.Graphics.Display.DisplayColorManagement.dll", "Windows.Internal.Graphics.Display.DisplayEnhancementManagement.dll", "Windows.Internal.HardwareConfirmator.dll", "Windows.Internal.Management.dll", "Windows.Internal.OpenWithHost.dll", "Windows.Internal.PlatformExtension.DevicePickerExperience.dll", "Windows.Internal.PlatformExtension.MiracastBannerExperience.dll", "Windows.Internal.PredictionUnit.dll", "Windows.Internal.Security.Attestation.DeviceAttestation.dll", "Windows.Internal.SecurityMitigationsBroker.dll", "Windows.Internal.Shell.Broker.dll", "Windows.Internal.Shell.CloudDesktop.TransitionScreen.dll", "Windows.Internal.Shell.XamlInputViewHost.dll", "windows.internal.shellcommon.AccountsControlExperience.dll", "windows.internal.shellcommon.AppResolverModal.dll", "Windows.Internal.ShellCommon.Broker.dll", "Windows.Internal.ShellCommon.dll", "windows.internal.shellcommon.FilePickerExperienceMEM.dll", "Windows.Internal.ShellCommon.PrintExperience.dll", "windows.internal.shellcommon.shareexperience.dll", "windows.internal.shellcommon.TokenBrokerModal.dll", "Windows.Internal.Signals.dll", "Windows.Internal.System.UserProfile.dll", "Windows.Internal.Taskbar.dll", "Windows.Internal.UI.BioEnrollment.ProxyStub.dll", "Windows.Internal.UI.Dialogs.dll", "Windows.Internal.UI.Logon.ProxyStub.dll", "Windows.Internal.UI.Shell.WindowTabManager.dll", "Windows.Internal.WaaSMedicDocked.dll", "Windows.Management.EnrollmentStatusTracking.ConfigProvider.dll", "Windows.Management.InprocObjects.dll", "Windows.Management.ModernDeployment.ConfigProviders.dll", "Windows.Management.Provisioning.ProxyStub.dll", "Windows.Management.Service.dll", "Windows.Management.Update.dll", "Windows.Management.Workplace.dll", "Windows.Management.Workplace.WorkplaceSettings.dll", "Windows.Media.Audio.dll", "Windows.Media.BackgroundMediaPlayback.dll", "Windows.Media.Devices.dll", "Windows.Media.dll", "Windows.Media.Editing.dll", "Windows.Media.FaceAnalysis.dll", "Windows.Media.Import.dll", "Windows.Media.MediaControl.dll", "Windows.Media.MixedRealityCapture.dll", "Windows.Media.Ocr.dll", "Windows.Media.Playback.BackgroundMediaPlayer.dll", "Windows.Media.Playback.MediaPlayer.dll", "Windows.Media.Playback.ProxyStub.dll", "Windows.Media.Protection.PlayReady.dll", "Windows.Media.Renewal.dll", "Windows.Media.Speech.dll", "Windows.Media.Speech.UXRes.dll", "Windows.Media.Streaming.dll", "Windows.Media.Streaming.ps.dll", "Windows.Mirage.dll", "Windows.Mirage.Internal.dll", "Windows.Networking.BackgroundTransfer.BackgroundManagerPolicy.dll", "Windows.Networking.BackgroundTransfer.ContentPrefetchTask.dll", "Windows.Networking.BackgroundTransfer.dll", "Windows.Networking.Connectivity.dll", "Windows.Networking.dll", "Windows.Networking.HostName.dll", "Windows.Networking.NetworkOperators.ESim.dll", "Windows.Networking.NetworkOperators.HotspotAuthentication.dll", "Windows.Networking.Proximity.dll", "Windows.Networking.ServiceDiscovery.Dnssd.dll", "Windows.Networking.Sockets.PushEnabledApplication.dll", "Windows.Networking.UX.EapRequestHandler.dll", "Windows.Networking.Vpn.dll", "Windows.Networking.XboxLive.ProxyStub.dll", "Windows.Payments.dll", "Windows.Perception.Stub.dll", "Windows.Security.Authentication.Identity.Provider.dll", "Windows.Security.Authentication.OnlineId.dll", "Windows.Security.Authentication.Web.Core.dll", "Windows.Security.Credentials.UI.CredentialPicker.dll", "Windows.Security.Credentials.UI.UserConsentVerifier.dll", "Windows.Security.Integrity.dll", "Windows.Services.TargetedContent.dll", "Windows.SharedPC.AccountManager.dll", "Windows.SharedPC.CredentialProvider.dll", "Windows.Shell.BlueLightReduction.dll", "Windows.Shell.ServiceHostBuilder.dll", "Windows.Shell.StartLayoutPopulationEvents.dll", "Windows.StateRepository.dll", "Windows.StateRepositoryBroker.dll", "Windows.StateRepositoryClient.dll", "Windows.StateRepositoryCore.dll", "Windows.StateRepositoryPS.dll", "Windows.StateRepositoryUpgrade.dll", "Windows.Storage.ApplicationData.dll", "Windows.Storage.Compression.dll", "windows.storage.dll", "Windows.Storage.OneCore.dll", "Windows.Storage.Search.dll", "Windows.System.Diagnostics.dll", "Windows.System.Diagnostics.Telemetry.PlatformTelemetryClient.dll", "Windows.System.Diagnostics.TraceReporting.PlatformDiagnosticActions.dll", "Windows.System.Launcher.dll", "Windows.System.Profile.HardwareId.dll", "Windows.System.Profile.PlatformDiagnosticsAndUsageDataSettings.dll", "Windows.System.Profile.RetailInfo.dll", "Windows.System.Profile.SystemId.dll", "Windows.System.Profile.SystemManufacturers.dll", "Windows.System.RemoteDesktop.dll", "Windows.System.SystemManagement.dll", "Windows.System.UserDeviceAssociation.dll", "Windows.System.UserProfile.DiagnosticsSettings.dll", "Windows.UI.Accessibility.dll", "Windows.UI.AppDefaults.dll", "Windows.UI.BioFeedback.dll", "Windows.UI.BlockedShutdown.dll", "Windows.UI.Core.TextInput.dll", "Windows.UI.Cred.dll", "Windows.UI.CredDialogController.dll", "Windows.UI.dll", "Windows.UI.FileExplorer.dll", "Windows.UI.Immersive.dll", "Windows.UI.Input.Inking.Analysis.dll", "Windows.UI.Input.Inking.dll", "Windows.UI.Logon.dll", "Windows.UI.NetworkUXController.dll", "Windows.UI.PicturePassword.dll", "Windows.UI.Search.dll", "Windows.UI.Shell.dll", "Windows.UI.Shell.Internal.AdaptiveCards.dll", "Windows.UI.Storage.dll", "Windows.UI.Xaml.Controls.dll", "Windows.UI.Xaml.dll", "Windows.UI.Xaml.InkControls.dll", "Windows.UI.Xaml.Maps.dll", "Windows.UI.Xaml.Phone.dll", "Windows.UI.Xaml.Resources.19h1.dll", "Windows.UI.Xaml.Resources.21h1.dll", "Windows.UI.Xaml.Resources.Common.dll", "Windows.UI.Xaml.Resources.rs1.dll", "Windows.UI.Xaml.Resources.rs2.dll", "Windows.UI.Xaml.Resources.rs3.dll", "Windows.UI.Xaml.Resources.rs4.dll", "Windows.UI.Xaml.Resources.rs5.dll", "Windows.UI.Xaml.Resources.th.dll", "Windows.UI.Xaml.Resources.win81.dll", "Windows.UI.Xaml.Resources.win8rtm.dll", "Windows.UI.XamlHost.dll", "Windows.WARP.JITService.dll", "Windows.Web.Diagnostics.dll", "Windows.Web.dll", "Windows.Web.Http.dll", "WindowsAccessBridge-64.dll", "WindowsCodecs.dll", "WindowsCodecsExt.dll", "WindowsDefaultHeatProcessor.dll", "WindowsInternal.ComposableShell.Display.dll", "WindowsInternal.Shell.CompUiActivation.dll", "windowslivelogin.dll", "WindowsManagementServiceWinRt.ProxyStub.dll", "windowsperformancerecordercontrol.dll", "windowsudk.shellcommon.dll", "windowsudkservices.shellcommon.dll", "winethc.dll", "WinFax.dll", "winhttp.dll", "winhttpcom.dll", "WinHvEmulation.dll", "WinHvPlatform.dll", "wininet.dll", "wininetlui.dll", "wininitext.dll", "winipcfile.dll", "winipcsecproc.dll", "winipsec.dll", "Winlangdb.dll", "winlogonext.dll", "winmde.dll", "winml.dll", "winmm.dll", "winmmbase.dll", "winmsipc.dll", "WinMsoIrmProtector.dll", "winnlsres.dll", "winnsi.dll", "WinOpcIrmProtector.dll", "WinREAgent.dll", "winrnr.dll", "winrscmd.dll", "winrsmgr.dll", "winrssrv.dll", "WinRtTracing.dll", "WinSATAPI.dll", "WinSCard.dll", "winshfhc.dll", "winsku.dll", "winsockhc.dll", "winsqlite3.dll", "winsrv.dll", "winsrvext.dll", "winsta.dll", "WinSync.dll", "WinSyncMetastore.dll", "WinSyncProviders.dll", "wintrust.dll", "WinTypes.dll", "WinUICohabitation.dll", "winusb.dll", "WinUSBCoInstaller2.dll", "WiredNetworkCSP.dll", "wisp.dll", "witnesswmiv2provider.dll", "wkscli.dll", "wkspbrokerAx.dll", "wksprtPS.dll", "wkssvc.dll", "wlanapi.dll", "wlancfg.dll", "WLanConn.dll", "wlandlg.dll", "wlangpui.dll", "WLanHC.dll", "wlanhlp.dll", "WlanMediaManager.dll", "WlanMM.dll", "wlanmsm.dll", "wlanpref.dll", "WlanRadioManager.dll", "wlansec.dll", "wlansvc.dll", "wlansvcpal.dll", "wlanui.dll", "wlanutil.dll", "Wldap32.dll", "wldp.dll", "wlgpclnt.dll", "wlidcli.dll", "wlidcredprov.dll", "wlidfdp.dll", "wlidnsp.dll", "wlidprov.dll", "wlidres.dll", "wlidsvc.dll", "WMALFXGFXDSP.dll", "wmcodecdspps.dll", "wmdmlog.dll", "wmdmps.dll", "wmdrmsdk.dll", "wmerror.dll", "wmi.dll", "wmiclnt.dll", "wmidcom.dll", "wmidx.dll", "wmiprop.dll", "wmitomi.dll", "WMNetMgr.dll", "wmp.dll", "WmpDui.dll", "wmpdxm.dll", "wmpeffects.dll", "WMPhoto.dll", "wmpps.dll", "wmpshell.dll", "wmsgapi.dll", "wmvdspa.dll", "WofTasks.dll", "WofUtil.dll", "WordBreakers.dll", "WorkfoldersControl.dll", "WorkFoldersGPExt.dll", "WorkFoldersRes.dll", "WorkFoldersShell.dll", "workfolderssvc.dll", "wosc.dll", "wow64.dll", "wow64base.dll", "wow64con.dll", "wow64cpu.dll", "wow64win.dll", "wpbcreds.dll", "Wpc.dll", "WpcApi.dll", "WpcDesktopMonSvc.dll", "WpcProxyStubs.dll", "WpcRefreshTask.dll", "WpcWebFilter.dll", "wpdbusenum.dll", "WpdMtp.dll", "WpdMtpUS.dll", "wpdshext.dll", "WPDShServiceObj.dll", "WPDSp.dll", "wpd_ci.dll", "wpnapps.dll", "wpnclient.dll", "wpncore.dll", "wpninprc.dll", "wpnprv.dll", "wpnservice.dll", "wpnsruprov.dll", "WpnUserService.dll", "WpPortingLibrary.dll", "WppRecorderUM.dll", "WPTaskScheduler.dll", "wpx.dll", "ws2help.dll", "ws2_32.dll", "wscapi.dll", "wscinterop.dll", "wscisvif.dll", "WSClient.dll", "wscproxystub.dll", "wscsvc.dll", "WSDApi.dll", "wsdchngr.dll", "WsdProviderUtil.dll", "WSDScanProxy.dll", "wsecedit.dll", "wsepno.dll", "wshbth.dll", "wshcon.dll", "wshelper.dll", "wshext.dll", "wshhyperv.dll", "wship6.dll", "wshqos.dll", "wshrm.dll", "wshunix.dll", "wslapi.dll", "WsmAgent.dll", "WSManMigrationPlugin.dll", "WsmAuto.dll", "wsmplpxy.dll", "WsmRes.dll", "WsmSvc.dll", "WsmWmiPl.dll", "wsnmp32.dll", "wsock32.dll", "wsplib.dll", "wsp_fs.dll", "wsp_health.dll", "wsp_sr.dll", "wtdccm.dll", "wtdhost.dll", "wtdsensor.dll", "wtsapi32.dll", "wuapi.dll", "wuaueng.dll", "wuceffects.dll", "WUDFCoinstaller.dll", "WUDFPlatform.dll", "WudfSMCClassExt.dll", "WUDFx.dll", "WUDFx02000.dll", "wudriver.dll", "wups.dll", "wups2.dll", "wusys.dll", "wvc.dll", "WwaApi.dll", "WwaExt.dll", "WWanAPI.dll", "wwancfg.dll", "WWanHC.dll", "WwanPrfl.dll", "wwanprotdim.dll", "WwanRadioManager.dll", "wwansvc.dll", "wwapi.dll", "x3daudio1_0.dll", "X3DAudio1_7.dll", "xactengine2_1.dll", "xactengine3_7.dll", "XamlTileRender.dll", "XAPOFX1_5.dll", "XAudio2_7.dll", "XAudio2_8.dll", "XAudio2_9.dll", "XblAuthManager.dll", "XblAuthManagerProxy.dll", "XblAuthTokenBrokerExt.dll", "XblGameSave.dll", "XblGameSaveExt.dll", "XblGameSaveProxy.dll", "XboxGipRadioManager.dll", "xboxgipsvc.dll", "xboxgipsynthetic.dll", "XboxNetApiSvc.dll", "xgameruntime.dll", "xinput1_1.dll", "XInput1_4.dll", "XInput9_1_0.dll", "XInputUap.dll", "xmlfilter.dll", "xmllite.dll", "xmlprovi.dll", "xolehlp.dll", "XpsDocumentTargetPrint.dll", "XpsGdiConverter.dll", "XpsPrint.dll", "xpspushlayer.dll", "XpsRasterService.dll", "xpsservices.dll", "XpsToPclmConverter.dll", "XpsToPwgrConverter.dll", "xwizards.dll", "xwreg.dll", "xwtpdui.dll", "xwtpw32.dll", "ze_loader.dll", "ze_tracing_layer.dll", "ze_validation_layer.dll", "zipcontainer.dll", "zipfldr.dll", "ztrace_maps.dll", "_SecEditCtl.BCM.x64.dll"];

        let name = name.to_lowercase();
        for sys in SYSTEM_DLL_LIST {
            if sys.to_lowercase() == name {
                return true;
            }
        }
        false
    };
}

fn can_be_dir<P: AsRef<Path>>(path: &P) -> bool {
    if let Ok(md) = std::fs::metadata(path) {
        if md.is_dir() {
            return true;
        }
    }
    return false;
}

fn is_file<P: AsRef<Path>>(path: &P) -> bool {
    if let Ok(md) = std::fs::metadata(path) {
        return md.is_file();
    }
    return false;
}

fn get_file_format(filename: &str, objdump_loc: &str) -> String {
    let output = Command::new(objdump_loc).args(["-f", filename]).output().unwrap();

    if !output.status.success() {
        let command = format!("{objdump_loc} -f {filename}");
        eprintln!("{command} failed with error code {:?}", output.status.to_string());
        eprintln!("It failed with std error output: \n{}", String::from_utf8(output.stderr).unwrap());
        exit(1);
    }

    let output = String::from_utf8(output.stdout).unwrap().replace('\r', "");


    //println!("{}",output);
    for line in output.split("\n") {
        //println!("Line {idx}: {line}");
        if let Some(loc) = line.rfind("file format ") {
            let loc = loc + "file format ".len();
            return line[loc..line.len()].to_string();
        }
    }
    eprintln!("Failed to parse file format of {filename} from objdump output, it says: \n{output}");
    exit(1);
}

fn validate_dll(dll_loc: &Path, args: &Args, custom_validator: Option<&dyn Fn(&Path) -> Result<(), String>>) -> bool {
    if !is_file(&dll_loc) {
        return false;
    }
    if let Some(validate) = &custom_validator {
        if let Err(reason) = validate(&dll_loc) {
            if args.verbose {
                println!("Skipped \"{}\" because {reason}", dll_loc.display());
            }
            return false;
        }
    }
    return true;
}

fn search_dll_deep(name: &str, args: &Args, validate: Option<&dyn Fn(&Path) -> Result<(), String>>) -> Option<String> {
    use walkdir::WalkDir;
    for dir in args.deep_search_dirs() {
        for entry in WalkDir::new(dir) {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    if args.verbose {
                        println!("Failed to search in \"{:?}\" because {}", e.path(), e);
                    }
                    continue;
                }
            };
            let mut loc = entry.path().to_path_buf();
            loc.push(name);

            if !validate_dll(&loc, args, validate) {
                continue;
            }

            return Some(loc.to_str().unwrap().to_string());
        }
    }

    return None;
}


fn search_dll_shallow(name: &str, args: &Args, validate: Option<&dyn Fn(&Path) -> Result<(), String>>) -> Option<String> {
    for path in args.shallow_search_dirs() {
        let mut loc = PathBuf::from(path);
        loc.push(name);

        if !validate_dll(&loc, args, validate) {
            continue;
        }

        return Some(loc.to_str().unwrap().to_string());
    }
    return None;
}


fn deploy_dll(target_binary: &str, target_dir: &str, objdump_file: &str, binary_format: &str, args: &Args) {
    if args.verbose {
        println!("Deploying for \"{target_binary}\" at \"{target_dir}\"");
    }
    let deps = get_dependencies(target_binary, objdump_file);

    for dep in &deps {
        let expected_filename = format!("{target_dir}/{dep}");
        if let Ok(_) = std::fs::metadata(&expected_filename) {
            // the dll already exist
            if args.verbose {
                println!("{expected_filename} already exists");
            }
            continue;
        }

        if args.ignore.contains(&dep) {
            // The dll is assigned to be ignored
            if args.verbose {
                println!("Skip {dep} because it is assigned to be ignored");
            }
            continue;
        }


        if is_system_dll(dep) {
            // Skip system dll
            if args.verbose {
                println!("Skip system dll {dep}");
            }
            continue;
        }

        if !args.copy_vc_redist && is_vc_redist_dll(dep) {
            // Skip vc redist dll.
            if args.verbose {
                println!("Skip VC redistributable dll {dep}");
            }
            continue;
        }


        if args.verbose {
            println!("Searching {dep} for {target_binary}");
        }
        // search for it
        let mut loc = None;

        let validator = |loc: &Path| {
            let format = get_file_format(loc.to_str().unwrap(), objdump_file);
            if format != binary_format {
                return Err(format!("DLL architecture mismatch. Expected {binary_format}, but found {format}"));
            }
            return Ok(());
        };
        let validator = Box::new(validator);

        // try shallow search first
        if let None = &loc {
            if !args.no_shallow_search {
                if let Some(location) = search_dll_shallow(dep, args, Some(&validator)) {
                    loc = Some(location);
                }
            }
        }
        if let None = &loc {
            if !args.no_deep_search {
                if let Some(location) = search_dll_deep(dep, args, Some(&validator)) {
                    loc = Some(location);
                }
            }
        }

        if let Some(location) = &loc {
            if args.verbose {
                println!("Copying \"{location}\" to \"{target_dir}\"");
            }
            std::fs::copy(location, &expected_filename).expect("Failed to copy dll");
        } else if args.allow_missing {
            println!("Failed to find dll \"{dep}\", required by \"{target_binary}\"");
            continue;
        } else {
            eprintln!("Failed to find dll \"{dep}\", required by \"{target_binary}\"");
            exit(1);
        }


        deploy_dll(&expected_filename, target_dir, objdump_file, binary_format, args);
    }
}

fn main() {
    let mut args = Args::parse();
    {
        let target = PathBuf::from(&args.binary_file);
        if !is_file(&target) {
            eprintln!("The given target \"{}\" is not a file",target.display());
            exit(5);
        }
        if target.is_relative() {
            if args.verbose {
                print!("The given binary path \"{}\" is a relative path, ", &args.binary_file);
            }
            let mut new_target = std::env::current_dir().unwrap();
            new_target.push(target);
            let new_target = new_target.to_str().unwrap().to_string();
            if args.verbose {
                println!("converted to \"{new_target}\"")
            }
            args.binary_file = new_target;
            assert!(is_file(&args.binary_file));
        }
    }

    let objdump_loc=args.objdump_file();
    if args.verbose {
        println!("Using objdump at {objdump_loc}");
    }

    let target_dir = PathBuf::from(&args.binary_file);
    let target_dir = target_dir.parent().unwrap().to_str().unwrap();
    let format = get_file_format(&args.binary_file, &args.objdump_file());
    if args.verbose {
        println!("Binary format: \"{format}\"");
    }
    deploy_dll(&args.binary_file, target_dir, &objdump_loc, &format, &args);
}
