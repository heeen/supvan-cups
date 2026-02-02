// =====================================================================
// Katasymbol Label Printer - Key Functions Extracted from Minified Bundle
// Source: app.77a7833b.js
// Extracted: 2026-02-01T10:27:38.213Z
// =====================================================================

// =====================================================================
// SECTION: COMMAND OPCODE CONSTANTS
// Command opcodes used in the printer protocol
// (6 matches)
// =====================================================================

// --- CMD_CONSTANTS_OBJECT [ObjectExpression with CMD constants] (line 3, col 535814) ---
{
  CMD_BUF_FULL: 16,
  CMD_INQUIRY_STA: 17,
  CMD_CHECK_DEVICE: 18,
  CMD_STATR_PRINT: 19,
  CMD_NEXTFRM_FIRMWARE_BULK: 198,
  CMD_STOP_PRINT: 20,
  CMD_RETURN_MAT: 48,
  CMD_RETURN_DEVICE: 8888,
  CMD_NEXTFRM_BULK: 92,
  CMD_READ_FIRMWARE_REV: 197,
  CMD_TRANSFER: 240,
  CMD_TRANSFER_ONE: 888,
  CMD_SET_RFID_DATA: 93,
  CMD_SET_RFID_DATA_WRITE: 999,
  CMD_READ_REV: 23,
  ERROR_CMD_INQUIRY_STA: 123,
  dataLength: 500,
  nCnt: 0,
  step: 0,
  n: 0,
  mCurrentCommand: 0,
  isTransferPart: !1,
  partIndex: 0,
  count: 0,
  byteArrayList: [],
  imageDataListAll: [],
  imageDataList: [],
  isStop: !1,
  RfidData: [],
  printerSn: "",
  paperType: 1,
  gap: 8,
  speed: 60,
  noInstallType: !1,
  isNetWork: !1,
  copiesAll: 0,
  mMatWidth: 0,
  mMatHeiht: 0,
  isPrintFlow: 0,
  statusNum: 0,
  errorMessage: "",
  waitImageNum: 0,
  waitStopPrintNum: 0,
  waitEndPrintNum: 0,
  objectLength: 0,
  sendCmdState: !1,
  sendCmdNum: 0,
  waitSendCmdNum: 0,
  sedMatrixNum: 0,
  cmdBufFullNum: 0,
  sendRfidDataNum: 0,
  waitComOkNum: 20,
  checkStopPrintNum: 20,
  pageObject: {},
  sendDataNum: 50,
  imageData: [],
  startPrint(A, e) {
    const t = this;
    t.isStop = !1, t.statusNum = 100, this.waitImageNum = 0, this.imageDataList = a["a"].state.project.currentPrint.imageDataListAll.shift(), this.RfidData = [], this.objectLength = e, t.sendCmdNum = 0, t.cmdBufFullNum = 0, t.waitComOkNum = 20, t.checkStopPrintNum = 20, t.sedMatrixNum = 0, this.waitStopPrintNum = 0, this.waitEndPrintNum = 0, this.pageObject = A, this.sendDataNum = 50, this.step = 1, t.handleStep();
  },
  sendCmd(A, e) {
    if (!this.isStop) return a["a"].state.project.currentPrint.isStop ? (this.isStop = !0, this.step = 14, void this.handleStep()) : void (a["a"].state.project.currentPrint.sendCmdState || g["a"].SendCmd(A, e));
  },
  handleStep() {
    const A = this;
    switch (A.step) {
      case 0:
        break;
      case 1:
        A.mCurrentCommand = A.CMD_INQUIRY_STA, A.sendCmd(A.mCurrentCommand, 0);
        break;
      case 2:
        A.mCurrentCommand = A.CMD_CHECK_DEVICE, A.sendCmd(A.mCurrentCommand, 0);
        break;
      case 3:
        A.waitComOk();
        break;
      case 4:
        if (A.devCheckErrMsg()) return void this.closePrint();
        A.startG();
        break;
      case 5:
        A.WaitDevRun();
        break;
      case 6:
        if (A.devCheckErrMsg()) return void this.closePrint();
        A.handleTransferStep();
        break;
      case 7:
        if (A.devCheckErrMsg()) return void this.closePrint();
        this.sendMatrix();
        break;
      case 8:
        if (A.devCheckErrMsg()) return void this.closePrint();
        this.cmdbuffull();
        break;
      case 9:
        if (A.devCheckErrMsg()) return void this.closePrint();
        A.handleTransferNext();
        break;
      case 10:
        if (A.devCheckErrMsg()) return void this.closePrint();
        A.waitNewPrint();
        break;
      case 11:
        A.printEnd();
        break;
      case 12:
        A.waitClose();
        break;
      case 13:
        A.closePrint();
        break;
      case 14:
        this.checkStopPrint();
        break;
      case 15:
        this.stopPrintManual();
        break;
    }
  },
  handleNotify(A) {
    switch (this.mCurrentCommand) {
      case this.CMD_INQUIRY_STA:
        if (this.handleInquiryStatus(A)) return 1 == this.step ? i.PrtSta ? void a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "设备忙!"
        }) : (this.step = 2, void this.handleStep()) : void this.handleStep();
        a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "检测设备失败!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
      case this.CMD_CHECK_DEVICE:
        if (this.handleInquiryStatus(A)) return this.step = 3, void this.handleStep();
        a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "检测设备失败!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
      case this.CMD_STATR_PRINT:
        if (this.handleInquiryStatus(A)) return this.step = 5, void this.handleStep();
        a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "启动打印失败"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
      case this.CMD_NEXTFRM_BULK:
        if (this.handleInquiryStatus(A)) return this.step = 7, void this.handleStep();
        a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "传输数据错误!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
      case this.CMD_TRANSFER_ONE:
        if (this.handleInquiryStatus(A)) return this.step = 8, void this.handleStep();
        a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "传输数据错误!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
      case this.CMD_BUF_FULL:
        if (console.log("******************接收到志满完成命令回复*****************************"), this.handleInquiryStatus(A)) {
          this.imageDataList != [] && this.imageDataList.length > 0 ? setTimeout(() => {
            this.sendDataNum = 50, this.step = 6, this.handleStep();
          }, 100) : 0 == this.isPrintFlow && (a["a"].state.project.currentPrint.imageDataListAll.length > 0 ? (this.imageDataList = [], this.imageDataList = a["a"].state.project.currentPrint.imageDataListAll.shift(), setTimeout(() => {
            this.handleTransferNext();
          }, 100)) : setTimeout(() => {
            this.waitImageNum = 20, this.waitImageList();
          }, 100));
          break;
        }
        a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "传输数据错误!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
      case this.CMD_RETURN_MAT:
        if (this.handleInquiryStatus(A)) {
          let e = Number.parseInt(A[31] + ((255 & A[32]) << 8));
          if (e > 0) {
            let t = this.byteToString(A, 11, 21),
              g = "118" + e.toString().padStart(3, "0"),
              i = Object(B["a"])(I["a"]);
            i.Sn = e, i.DeviceSn = t, i.LableSn = g, a["a"].commit("shareMemery/editIsCustomLable", {
              isCustomLable: !1
            }), a["a"].commit("mat/setAutoIdentifyMat", {
              autoIdentifyMat: i
            });
          }
        } else this.step = 0, this.mCurrentCommand = 0;
        this.closePrint();
        break;
      case this.CMD_STOP_PRINT:
        if (g["a"].handleInquiryStatus(A)) return this.step = 12, void this.handleStep();
        a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "状态查询命令失败!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
    }
  },
  handleInquiryStatus(A) {
    try {
      let e = new Uint8Array(8);
      e[0] = A[1], e[1] = A[2], e[2] = A[3], e[3] = A[4], e[4] = A[5], e[5] = A[6], e[6] = A[7], e[7] = A[8], i.Refresh(e);
    } catch (e) {
      return console.log(e), !1;
    }
    return !0;
  },
  getMatSn() {
    this.isStop = !1, a["a"].state.project.currentPrint.isStop = !1, this.mCurrentCommand = this.CMD_RETURN_MAT, this.sendCmd(this.mCurrentCommand, 0);
  },
  stopPrintManual() {
    const A = this;
    A.waitStopPrintNum > 50 ? a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "检测设备超时!"
    }) : a["a"].state.project.currentPrint.sendCmdState ? setTimeout(() => {
      A.waitStopPrintNum++, A.waitStopPrintManual(), console.log("设备忙碌中，等待设备空闲在发送命令");
    }, 500) : i.PrtSta ? (A.mCurrentCommand = A.CMD_STOP_PRINT, g["a"].SendCmd(A.mCurrentCommand, 0), console.log("发送终止命令")) : this.closePrint();
  },
  waitStopPrintManual() {
    const A = this;
    A.devCheckErrMsg(!1) || (i.PrtSta ? a["a"].state.project.currentPrint.sendCmdState ? (A.waitStopPrintManual(), console.log("设备忙碌中，等待设备空闲在发送命令")) : setTimeout(() => {
      g["a"].SendCmd(A.CMD_INQUIRY_STA, 0);
    }, 500) : this.closePrint());
  },
  handleNotifyStop(A) {
    g["a"].handleInquiryStatus(A) && this.waitStopPrintManual();
  },
  checkStopPrint() {
    const A = this;
    return A.mCurrentCommand == A.CMD_INQUIRY_STA ? (console.log("checkStopPrint", i.PrtSta), i.PrtSta ? (A.step = 15, void A.handleStep()) : (A.step = 12, void A.handleStep())) : A.checkStopPrintNum < 0 ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "检测设备超时!"
    }), A.step = 0, A.mCurrentCommand = 0, void A.closePrint()) : (console.log("重新检测", i.printingStation, a["a"].state.project.currentPrint.sendCmdState, A.step), void setTimeout(function () {
      A.checkStopPrintNum--, A.mCurrentCommand = A.CMD_INQUIRY_STA, g["a"].SendCmd(A.mCurrentCommand, 0);
    }, 500));
  },
  waitClose() {
    console.log(i.PrtSta, this.waitEndPrintNum), i.PrtSta && this.waitEndPrintNum < 20 ? setTimeout(() => {
      this.step = 12, this.mCurrentCommand = this.CMD_INQUIRY_STA, g["a"].SendCmd(this.mCurrentCommand, 0), this.waitEndPrintNum++;
    }, 200) : (this.mCurrentCommand = 0, this.closePrint());
  },
  printEnd() {
    this.closePrint();
  },
  waitNewPrint() {
    if (console.log(i.pPageCnt, this.objectLength - 1, i.PrtSta), i.pPageCnt > a["a"].state.project.currentPrint.num - a["a"].state.project.currentPrint.keepOnPrintPosition && this.objectLength > a["a"].state.project.currentPrint.num && a["a"].commit("project/currentPrintNumAdd"), i.pPageCnt >= this.objectLength - 1 || 1 == this.objectLength || !i.PrtSta) return console.log("关闭"), this.objectLength > a["a"].state.project.currentPrint.num && a["a"].commit("project/currentPrintNumAdd"), this.step = 11, this.mCurrentCommand = this.CMD_INQUIRY_STA, void this.sendCmd(this.CMD_INQUIRY_STA, 0);
    this.waitImageNum <= 0 || (a["a"].state.project.currentPrint.imageDataListAll.length > 0 ? (this.imageDataList = [], this.imageDataList = a["a"].state.project.currentPrint.imageDataListAll.shift(), setTimeout(() => {
      this.handleTransferNext();
    }, 100)) : setTimeout(() => {
      console.log("等待新的打印", this.waitImageNum), this.waitImageList(), this.waitImageNum--;
    }, 500));
  },
  waitImageList() {
    this.step = 10, this.mCurrentCommand = this.CMD_INQUIRY_STA, this.sendCmd(this.CMD_INQUIRY_STA, 0);
  },
  handleTransferNext() {
    if (i.pPageCnt > a["a"].state.project.currentPrint.num - a["a"].state.project.currentPrint.keepOnPrintPosition && this.objectLength > a["a"].state.project.currentPrint.num && a["a"].commit("project/currentPrintNumAdd"), a["a"].state.project.currentPrint.state) {
      if (this.devCheckErrMsg()) this.closePrint();else if (i.BufFull) {
        if (console.log("等待上一张打印完成"), !i.PrtSta) return a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "打印被终止!"
        }), void this.closePrint();
        setTimeout(() => {
          this.step = 9, this.mCurrentCommand = this.CMD_INQUIRY_STA, this.sendCmd(this.mCurrentCommand, 0);
        }, 50);
      } else console.log("打印下一张"), setTimeout(() => {
        this.step = 6, this.handleStep();
      }, 100);
    } else this.closePrint();
  },
  cmdbuffull() {
    console.log("进入置满", a["a"].state.project.currentPrint.sendCmdState), this.cmdBufFullNum > 50 ? console.log("设备超时，置满失败") : a["a"].state.project.currentPrint.sendCmdState || a["a"].state.project.currentPrint.sendCmdMatrix.num != a["a"].state.project.currentPrint.sendCmdMatrix.total ? (console.log("重试置满"), setTimeout(() => {
      this.cmdBufFullNum++, this.cmdbuffull();
    }, 100)) : setTimeout(() => {
      this.cmdBufFullNum = 0, a["a"].state.project.currentPrint.sendCmdMatrix.num = 0, a["a"].state.project.currentPrint.sendCmdMatrix.total = 0, this.n = 0, this.mCurrentCommand = this.CMD_BUF_FULL, g["a"].SendCmdTwo(this.mCurrentCommand, this.imageData.length, this.speed);
    }, 50);
  },
  sendMatrix() {
    this.sedMatrixNum > 50 ? console.log("设备超时，发送字模失败") : a["a"].state.project.currentPrint.sendCmdState ? (console.log("重试sedMatrix"), setTimeout(() => {
      this.sedMatrixNum++, this.sendMatrix();
    }, 200)) : setTimeout(() => {
      this.sedMatrixNum = 0, this.mCurrentCommand = this.CMD_TRANSFER_ONE, g["a"].BulkWriteType(this.imageData, this.imageData.length);
    }, 100);
  },
  handleTransferStep() {
    const A = this;
    if (!A.devCheckErrMsg()) return A.sendDataNum < 0 ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "状态查询命令失败"
    }), A.step = 0, A.mCurrentCommand = 0, A.sendDataNum = 50, void A.closePrint()) : void (A.mCurrentCommand == A.CMD_INQUIRY_STA && 0 == i.BufFull ? (A.sendDataNum = 50, A.imageData = A.imageDataList.shift(), A.imageDataList.length <= 0 && (console.log("渲染下一张"), a["a"].state.project.currentPrint.nextPrintState = !0), A.mCurrentCommand = A.CMD_NEXTFRM_BULK, A.sendCmd(A.CMD_NEXTFRM_BULK, A.imageData.length)) : (console.log("发送数据前检测"), setTimeout(() => {
      A.sendDataNum--, A.step = 6, A.mCurrentCommand = A.CMD_INQUIRY_STA, A.sendCmd(A.mCurrentCommand, 0);
    }, 100)));
    A.closePrint();
  },
  startG() {
    const A = this;
    A.mCurrentCommand = this.CMD_STATR_PRINT, this.sendCmd(A.mCurrentCommand, 0);
  },
  waitComOk() {
    const A = this;
    return A.mCurrentCommand == A.CMD_INQUIRY_STA && 0 == i.ComExeSta ? (A.step = 4, void A.handleStep()) : A.waitComOkNum < 0 ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "检测设备超时!"
    }), A.step = 0, A.mCurrentCommand = 0, void A.closePrint()) : void setTimeout(function () {
      A.waitComOkNum--, A.mCurrentCommand = A.CMD_INQUIRY_STA, A.sendCmd(A.mCurrentCommand, 0);
    }, 500);
  },
  WaitDevRun() {
    const A = this;
    return A.mCurrentCommand == A.CMD_INQUIRY_STA && i.PrtSta ? (A.step = 6, void A.handleStep()) : A.waitComOkNum < 0 ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "启动打印命令超时!"
    }), A.step = 0, A.mCurrentCommand = 0, void A.closePrint()) : void setTimeout(function () {
      A.waitComOkNum--, A.mCurrentCommand = A.CMD_INQUIRY_STA, A.sendCmd(A.mCurrentCommand, 0);
    }, 500);
  },
  closePrint() {
    const A = this;
    i.PrtSta ? setTimeout(function () {
      this.step = 13, console.log("等待设备释放", i.PrtSta), A.mCurrentCommand = A.CMD_INQUIRY_STA, g["a"].SendCmd(A.mCurrentCommand, 0);
    }, 500) : g["a"].hidDeviceClose().then(A => {
      this.hidDevice = A, console.log("设备已关闭", this.hidDevice, i.PrtSta), a["a"].state.project.currentPrint.isPrintEnd = !0, a["a"].state.project.currentPrint.close = !0;
    });
  },
  devCheckErrMsg() {
    return 1 == i.BatLow ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "电量低请充电"
    }), !0) : 1 == i.MatMoveOut ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "请安装标签"
    }), !0) : 1 == i.MatXhErr ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "标签未识别请更换"
    }), !0) : 1 == i.CoverOpen ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "上盖打开"
    }), !0) : 1 == i.MatOver ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "标签已用完请更换"
    }), !0) : 1 == i.MatFixErr || 1 == i.OptMatErr ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "标签未安装到位"
    }), !0) : 1 == i.OptErr ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "操作不当,打印终止"
    }), !0) : 1 == i.LabRwErr && (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "耗材异常"
    }), !0);
  },
  byteToString(A, e, t) {
    let g = [];
    for (let i = e; i < e + t; i++) {
      if (0 == A[i]) break;
      g.push(String.fromCharCode(A[i]));
    }
    return g.join("");
  }
}

// --- CMD_CONSTANTS_OBJECT [ObjectExpression with CMD constants] (line 3, col 2844067) ---
{
  CMD_BUF_FULL: 16,
  CMD_INQUIRY_STA: 18,
  CMD_STATR_PRINT: 19,
  CMD_STOP_PRINT: 20,
  CMD_CHECK_DEVICE: 24,
  CMD_RD_LAB_DPI24: 36,
  CMD_RD_LAB_DPI25: 37,
  CMDUSB_SET_MAT: 26,
  CMD_NEXTFRM_BULK: 92,
  CMD_TRANSFER_ONE: 888,
  imageDataList: [],
  isStop: !1,
  mCurrentCommand: 0,
  step: 0,
  waitComOkNum: 20,
  maxDotValue: 0,
  checkPrtStaNum: 50,
  sendDataNum: 50,
  pageObject: {},
  sedMatrixNum: 0,
  cmdBufFullNum: 0,
  speed: 60,
  waitImageNum: 0,
  objectLength: 0,
  imageData: [],
  waitStopPrintNum: 0,
  startPrint(A, e) {
    this.isStop = !1, this.waitComOkNum = 20, this.checkPrtStaNum = 50, this.sendDataNum = 50, this.sedMatrixNum = 0, this.cmdBufFullNum = 0, this.waitImageNum = 0, this.imageDataList = a["a"].state.project.currentPrint.imageDataListAll.shift(), this.objectLength = e, this.pageObject = A, this.step = 1, this.handleStep();
  },
  sendCmd(A, e) {
    if (!this.isStop) return a["a"].state.project.currentPrint.isStop ? (this.isStop = !0, void this.stopPrintManual()) : void (a["a"].state.project.currentPrint.sendCmdState || g["a"].SendCmd(A, e));
  },
  stopPrintManual() {
    const A = this;
    A.waitStopPrintNum > 50 ? console.log("设备超时，终止失败") : a["a"].state.project.currentPrint.sendCmdState ? setTimeout(() => {
      A.waitStopPrintNum++, A.waitStopPrintManual(), console.log("设备忙碌中，等待设备空闲在发送命令");
    }, 500) : i.fStaReg.PrtSta ? (g["a"].SendCmd(A.CMD_STOP_PRINT, 0), console.log("发送终止命令")) : this.closePrint();
  },
  waitStopPrintManual() {
    const A = this;
    A.devCheckErrMsg() ? this.closePrint() : i.fStaReg.PrtSta ? a["a"].state.project.currentPrint.sendCmdState ? (A.waitStopPrintManual(), console.log("设备忙碌中，等待设备空闲在发送命令")) : setTimeout(() => {
      g["a"].SendCmd(A.CMD_INQUIRY_STA, 0);
    }, 500) : this.closePrint();
  },
  handleNotifyStop(A) {
    this.handleInquiryStatus(A) && this.waitStopPrintManual();
  },
  handleNotify(A) {
    switch (this.mCurrentCommand) {
      case this.CMD_CHECK_DEVICE:
        if (this.handleInquiryStatus(A)) return this.step = 2, void this.handleStep();
        a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "状态查询命令失败!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
      case this.CMD_INQUIRY_STA:
        if (this.handleInquiryStatus(A)) return void this.handleStep();
        a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "设备检测命令超时"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
      case this.CMDUSB_SET_MAT:
        this.handleInquiryStatus(A) ? (this.step = 6, this.handleStep()) : (a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "设置材料失败!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint());
        break;
      case this.CMD_STATR_PRINT:
        this.handleInquiryStatus(A) ? (this.step = 7, this.handleStep()) : (a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "启动打印失败"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint());
        break;
      case this.CMD_NEXTFRM_BULK:
        this.handleInquiryStatus(A) ? (this.step = 10, this.handleStep()) : (a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "传输数据错误!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint());
        break;
      case this.CMD_TRANSFER_ONE:
        this.handleInquiryStatus(A) ? (this.step = 11, this.handleStep()) : (a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "传输数据错误!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint());
        break;
      case this.CMD_BUF_FULL:
        console.log("******************接收到志满完成命令回复*****************************"), this.imageDataList != [] && this.imageDataList.length > 0 ? setTimeout(() => {
          this.step = 9, this.handleStep();
        }, 100) : (console.log(a["a"].state.project.currentPrint.imageDataListAll), a["a"].state.project.currentPrint.imageDataListAll.length > 0 ? (this.imageDataList = [], this.imageDataList = a["a"].state.project.currentPrint.imageDataListAll.shift(), setTimeout(() => {
          this.handleTransferNext();
        }, 100)) : setTimeout(() => {
          this.waitImageNum = 100, this.waitImageList();
        }, 100));
        break;
      case this.CMD_RD_LAB_DPI24:
        this.handleInquiryStatus(A) ? (this.pageObject.DPI = 672 / (i.SendData / 10), (this.pageObject.DPI > 12 || this.pageObject.DPI < 11) && (this.pageObject.DPI = 11.8), a["a"].commit("project/editCurrentPrintPrintDPI", {
          DPI: this.pageObject.DPI
        }), this.closePrint()) : (a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "状态查询命令失败!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint());
        break;
      case this.CMD_RD_LAB_DPI25:
        this.handleInquiryStatus(A) ? (this.pageObject.DPI = 672 / (i.SendData / 10), (this.pageObject.DPI > 12 || this.pageObject.DPI < 11) && (this.pageObject.DPI = 11.8), a["a"].commit("project/editCurrentPrintPrintDPI", {
          DPI: this.pageObject.DPI
        }), this.closePrint()) : (a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "状态查询命令失败!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint());
        break;
    }
  },
  handleStep() {
    const A = this;
    switch (A.step) {
      case 0:
        break;
      case 1:
        A.mCurrentCommand = A.CMD_CHECK_DEVICE, A.sendCmd(A.mCurrentCommand, 0);
        break;
      case 2:
        A.waitComOk();
        break;
      case 3:
        if (A.devCheckErrMsg()) return void this.closePrint();
        A.needClr();
        break;
      case 4:
        break;
      case 5:
        A.sendPrintMaterial();
        break;
      case 6:
        A.startSP();
        break;
      case 7:
        A.checkPrtSta();
        break;
      case 8:
        A.spPrintImage();
        break;
      case 9:
        A.handleTransferStep();
        break;
      case 10:
        A.sendMatrix();
        break;
      case 11:
        A.cmdbuffull();
        break;
      case 12:
        A.waitNewPrint();
        break;
      case 13:
        A.printEnd();
        break;
      case 14:
        A.handleTransferNext();
        break;
    }
  },
  printEnd() {
    this.closePrint();
  },
  waitNewPrint() {
    if (!i.fStaReg.PrtSta) return i.pPageCnt > a["a"].state.project.currentPrint.num - a["a"].state.project.currentPrint.keepOnPrintPosition && this.objectLength > a["a"].state.project.currentPrint.num && a["a"].commit("project/currentPrintNumAdd"), this.step = 13, this.mCurrentCommand = this.CMD_INQUIRY_STA, void this.sendCmd(this.CMD_INQUIRY_STA, 0);
    this.devCheckErrMsg() ? this.closePrint() : this.waitImageNum <= 0 || (a["a"].state.project.currentPrint.imageDataListAll.length > 0 ? (this.imageDataList = [], this.imageDataList = a["a"].state.project.currentPrint.imageDataListAll.shift(), setTimeout(() => {
      this.handleTransferNext();
    }, 100)) : setTimeout(() => {
      console.log("等待新的打印", this.waitImageNum), this.waitImageList(), this.waitImageNum--;
    }, 500));
  },
  waitImageList() {
    this.step = 12, this.mCurrentCommand = this.CMD_INQUIRY_STA, this.sendCmd(this.CMD_INQUIRY_STA, 0);
  },
  handleTransferNext() {
    a["a"].state.project.currentPrint.state && (this.devCheckErrMsg() ? this.closePrint() : a["a"].state.project.currentPrint.num - a["a"].state.project.currentPrint.keepOnPrintPosition == i.pPageCnt ? setTimeout(() => {
      if (console.log("等待上一张打印完成", a["a"].state.project.currentPrint.num, a["a"].state.project.currentPrint.keepOnPrintPosition, i.pPageCnt), !i.fStaReg.PrtSta) return a["a"].commit("project/editCurrentPrintState", {
        state: !1,
        errorMsg: "打印被终止!"
      }), this.step = 0, this.mCurrentCommand = 0, void this.closePrint();
      this.step = 14, this.mCurrentCommand = this.CMD_INQUIRY_STA, this.sendCmd(this.mCurrentCommand, 0);
    }, 100) : (i.pPageCnt > a["a"].state.project.currentPrint.num - a["a"].state.project.currentPrint.keepOnPrintPosition && this.objectLength > a["a"].state.project.currentPrint.num && a["a"].commit("project/currentPrintNumAdd"), setTimeout(() => {
      this.step = 9, this.mCurrentCommand = this.CMD_INQUIRY_STA, this.sendCmd(this.mCurrentCommand, 0);
    }, 100)));
  },
  cmdbuffull() {
    console.log("进入置满", a["a"].state.project.currentPrint.sendCmdState), this.cmdBufFullNum > 50 ? console.log("设备超时，置满失败") : a["a"].state.project.currentPrint.sendCmdState || a["a"].state.project.currentPrint.sendCmdMatrix.num != a["a"].state.project.currentPrint.sendCmdMatrix.total ? (console.log("重试置满"), setTimeout(() => {
      this.cmdBufFullNum++, this.cmdbuffull();
    }, 200)) : setTimeout(() => {
      this.cmdBufFullNum = 0, a["a"].state.project.currentPrint.sendCmdMatrix.num = 0, a["a"].state.project.currentPrint.sendCmdMatrix.total = 0, this.n = 0, this.mCurrentCommand = this.CMD_BUF_FULL, this.sendCmd(this.mCurrentCommand, 0);
    }, 100);
  },
  sendMatrix() {
    this.sedMatrixNum > 50 ? console.log("设备超时，发送字模失败") : a["a"].state.project.currentPrint.sendCmdState ? (console.log("重试sedMatrix"), setTimeout(() => {
      this.sedMatrixNum++, this.sendMatrix();
    }, 200)) : setTimeout(() => {
      this.sedMatrixNum = 0, this.mCurrentCommand = this.CMD_TRANSFER_ONE, g["a"].BulkWriteType(this.imageData, this.imageData.length);
    }, 100);
  },
  handleTransferStep() {
    const A = this;
    if (!A.devCheckErrMsg()) return A.sendDataNum < 0 ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "状态查询命令失败"
    }), A.step = 0, A.mCurrentCommand = 0, A.sendDataNum = 50, void A.closePrint()) : void (A.mCurrentCommand == A.CMD_INQUIRY_STA && 0 == i.mStaReg.BufFull ? (A.sendDataNum = 50, A.imageData = A.imageDataList.shift(), A.imageDataList.length <= 0 && (console.log("渲染下一张"), a["a"].state.project.currentPrint.nextPrintState = !0), A.mCurrentCommand = A.CMD_NEXTFRM_BULK, A.sendCmd(A.CMD_NEXTFRM_BULK, A.imageData.length)) : (console.log("发送数据前检测"), setTimeout(() => {
      A.sendDataNum--, A.step = 9, A.mCurrentCommand = A.CMD_INQUIRY_STA, A.sendCmd(A.mCurrentCommand, 0);
    }, 100)));
    A.closePrint();
  },
  spPrintImage() {
    this.handleTransfer();
  },
  handleTransfer() {
    this.step = 9, this.mCurrentCommand = this.CMD_INQUIRY_STA, this.sendCmd(this.CMD_INQUIRY_STA, 0);
  },
  checkPrtSta() {
    const A = this;
    return console.log("启动检测", i.fStaReg.PrtSta, i.mStaReg.BufFull), A.mCurrentCommand == A.CMD_INQUIRY_STA && i.fStaReg.PrtSta ? (A.checkPrtStaNum = 50, this.step = 8, void this.handleStep()) : A.checkPrtStaNum < 0 ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "启动打印失败"
    }), A.step = 0, A.mCurrentCommand = 0, void A.closePrint()) : void setTimeout(function () {
      A.checkPrtStaNum--, A.mCurrentCommand = A.CMD_INQUIRY_STA, A.sendCmd(A.mCurrentCommand, 0);
    }, 100);
  },
  startSP() {
    console.log("启动打印");
    let A = this.getSPMaterialTypeCode(this.pageObject.PaperType);
    this.mCurrentCommand = this.CMD_STATR_PRINT, this.sendCmd(this.mCurrentCommand, A);
  },
  sendPrintMaterial() {
    let A = this.getSPMaterialTypeCode(this.pageObject.PaperType);
    this.mCurrentCommand = this.CMDUSB_SET_MAT, this.sendCmd(this.mCurrentCommand, A);
  },
  getSPMaterialTypeCode(A) {
    return A == I["a"].Continuous ? 1 : A == I["a"].DieCut ? 2 : A == I["a"].Plate ? 3 : 1;
  },
  needClr() {
    const A = this;
    if (i.mStaReg.QjgNeedClr) return a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "清洁辊需要清洁!"
    }), A.step = 0, A.mCurrentCommand = 0, void A.closePrint();
    this.step = 5, this.handleStep();
  },
  waitComOk() {
    const A = this;
    return A.mCurrentCommand == A.CMD_INQUIRY_STA && 0 == i.mStaReg.ComExeSta ? (this.waitComOkNum = 20, A.step = 3, void A.handleStep()) : A.waitComOkNum < 0 ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "设备检测命令超时"
    }), A.step = 0, A.mCurrentCommand = 0, void A.closePrint()) : void setTimeout(function () {
      A.waitComOkNum--, A.mCurrentCommand = A.CMD_INQUIRY_STA, A.sendCmd(A.mCurrentCommand, 0);
    }, 500);
  },
  closePrint() {
    g["a"].hidDeviceClose().then(A => {
      this.hidDevice = A, console.log("设备已关闭", this.hidDevice), a["a"].state.project.currentPrint.isPrintEnd = !0, a["a"].state.project.currentPrint.close = !0;
    });
  },
  handleInquiryStatus(A) {
    let e = new Uint8Array(8);
    e[0] = A[1], e[1] = A[2], e[2] = A[3], e[3] = A[4], e[4] = A[5], e[5] = A[6], e[6] = A[7], e[7] = A[8];
    let t = e[1];
    return t <<= 8, t += e[0], i.mStaRegRefresh(t), t = e[3], t <<= 8, t += e[2], i.fStaRegRefresh(t), i.pPageCnt = e[5], i.pPageCnt <<= 8, i.pPageCnt += e[4], i.SendData = e[7], i.SendData <<= 8, i.SendData += e[6], !0;
  },
  devCheckErrMsg() {
    return i.mStaReg.RibEnd ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "色带用完"
    }), !0) : (i.mStaReg.ChkMatOk || a["a"].commit("project/editCurrentPrintState", {
      state: !0,
      errorMsg: "色带出错"
    }), i.mStaReg.RibRwErr && a["a"].commit("project/editCurrentPrintState", {
      state: !0,
      errorMsg: "色带出错"
    }), i.mStaReg.RibXhErr && a["a"].commit("project/editCurrentPrintState", {
      state: !0,
      errorMsg: "色带出错"
    }), i.mStaReg.SysErr || i.mStaReg.DMAErr ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "系统内部出错"
    }), !0) : i.fStaReg.CoverOpen ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "上盖打开"
    }), !0) : i.fStaReg.RibEnd ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "色带用完"
    }), !0) : i.fStaReg.LabEnd ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "贴纸用完"
    }), !0) : i.fStaReg.HeadGz ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "打印头故障"
    }), !0) : !!i.fStaReg.QjgGz && (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "清洁装置故障"
    }), !0));
  },
  getDpi() {
    if (this.maxDotValue = 672, a["a"].state.project.projectModel && a["a"].state.project.projectModel.MatModel) return a["a"].state.project.projectModel.MatModel.PaperType == I["a"].Plate ? (a["a"].state.project.currentPrint.isStop = !1, this.mCurrentCommand = this.CMD_RD_LAB_DPI24, void g["a"].SendCmd(this.mCurrentCommand, 0)) : (a["a"].state.project.currentPrint.isStop = !1, this.mCurrentCommand = this.CMD_RD_LAB_DPI25, void g["a"].SendCmd(this.mCurrentCommand, 0));
  }
}

// --- CMD_CONSTANTS_OBJECT [ObjectExpression with CMD constants] (line 3, col 3006428) ---
{
  CMD_BUF_FULL: 16,
  CMD_INQUIRY_STA: 17,
  CMD_STATR_PRINT: 19,
  CMD_FINISH_PRINT: 17,
  CMD_STOP_PRINT: 20,
  CMD_CHECK_DEVICE: 18,
  CMD_CHECK_RIB: 25,
  CMD_RESET_PRINT: 20,
  CMD_RD_LAB_DPI: 34,
  CMD_NEXTFRM_BULK: 90,
  imageDataList: [],
  isStop: !1,
  mCurrentCommand: 0,
  step: 0,
  waitComOkNum: 20,
  maxDotValue: 0,
  checkPrtStaNum: 50,
  pageObject: {},
  sedMatrixNum: 0,
  cmdBufFullNum: 0,
  speed: 60,
  waitImageNum: 0,
  objectLength: 0,
  imageData: [],
  waitStopPrintNum: 0,
  sendDataNum: 50,
  CMD_TRANSFER_ONE: 888,
  ma: 0,
  startPrint(A, e) {
    this.isStop = !1, this.waitComOkNum = 20, this.checkPrtStaNum = 20, this.sedMatrixNum = 0, this.cmdBufFullNum = 0, this.waitImageNum = 0, this.imageDataList = a["a"].state.project.currentPrint.imageDataListAll.shift(), this.objectLength = e, this.pageObject = A, this.sendDataNum = 50, this.step = 1, this.handleStep();
  },
  sendCmd(A, e) {
    if (!this.isStop) return a["a"].state.project.currentPrint.isStop ? (this.isStop = !0, void this.stopPrintManual()) : void (a["a"].state.project.currentPrint.sendCmdState || g["a"].SendCmd(A, e));
  },
  stopPrintManual() {
    const A = this;
    A.waitStopPrintNum > 50 ? console.log("设备超时，终止失败") : a["a"].state.project.currentPrint.sendCmdState ? setTimeout(() => {
      A.waitStopPrintNum++, A.waitStopPrintManual(), console.log("设备忙碌中，等待设备空闲在发送命令");
    }, 500) : i["a"].fStaReg.PrtSta ? (g["a"].SendCmd(A.CMD_STOP_PRINT, 0), console.log("发送终止命令")) : this.closePrint();
  },
  waitStopPrintManual() {
    const A = this;
    A.devCheckErrMsg() ? this.closePrint() : i["a"].fStaReg.PrtSta ? a["a"].state.project.currentPrint.sendCmdState ? (A.waitStopPrintManual(), console.log("设备忙碌中，等待设备空闲在发送命令")) : setTimeout(() => {
      g["a"].SendCmd(A.CMD_INQUIRY_STA, 0);
    }, 500) : this.closePrint();
  },
  handleNotifyStop(A) {
    this.handleInquiryStatus(A) && this.waitStopPrintManual();
  },
  handleStep() {
    const A = this;
    switch (A.step) {
      case 0:
        break;
      case 1:
        A.ma = Number.parseInt(I["a"].getTPMaterialTypeCode() << 8) + Number.parseInt(this.pageObject.CutType), A.mCurrentCommand = A.CMD_CHECK_DEVICE, A.sendCmd(A.mCurrentCommand, A.ma);
        break;
      case 2:
        A.waitComOk();
        break;
      case 3:
        if (A.devCheckErrMsg()) return void this.closePrint();
        this.step = 4, this.handleStep();
        break;
      case 4:
        A.startTP();
        break;
      case 5:
        A.checkPrtSta();
        break;
      case 6:
        A.tpPrintImage();
        break;
      case 7:
        A.handleTransferStep();
        break;
      case 8:
        A.sendMatrix();
        break;
      case 9:
        A.cmdbuffull();
        break;
      case 10:
        A.handleTransferNext();
        break;
      case 11:
        A.waitNewPrint();
        break;
      case 12:
        A.printEnd();
        break;
    }
  },
  handleNotify(A) {
    switch (this.mCurrentCommand) {
      case this.CMD_INQUIRY_STA:
        if (this.handleInquiryStatus(A)) return void this.handleStep();
        a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "状态查询命令失败!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
      case this.CMD_CHECK_DEVICE:
        if (this.handleInquiryStatus(A)) return this.step = 2, void this.handleStep();
        a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "检测设备失败!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
      case this.CMD_STATR_PRINT:
        this.handleInquiryStatus(A) ? (this.step = 5, this.handleStep()) : (a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "启动打印失败"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint());
        break;
      case this.CMD_NEXTFRM_BULK:
        this.handleInquiryStatus(A) ? (this.step = 8, this.handleStep()) : (a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "传输数据错误!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint());
        break;
      case this.CMD_TRANSFER_ONE:
        this.handleInquiryStatus(A) ? (this.step = 9, this.handleStep()) : (a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "传输数据错误!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint());
        break;
      case this.CMD_BUF_FULL:
        console.log("******************接收到志满完成命令回复*****************************"), this.imageDataList != [] && this.imageDataList.length > 0 ? setTimeout(() => {
          this.step = 7, this.handleStep();
        }, 10) : a["a"].state.project.currentPrint.imageDataListAll.length > 0 ? (this.imageDataList = [], this.imageDataList = a["a"].state.project.currentPrint.imageDataListAll.shift(), setTimeout(() => {
          this.checkPrtStaNext();
        }, 100)) : setTimeout(() => {
          this.waitImageNum = 300, this.waitImageList();
        }, 100);
        break;
      case this.CMD_RD_LAB_DPI:
        if (this.handleInquiryStatus(A)) {
          let e = new Uint8Array(8);
          e[0] = A[1], e[1] = A[2], e[2] = A[3], e[3] = A[4], e[4] = A[5], e[5] = A[6], e[6] = A[7], e[7] = A[8];
          let t = a["a"].state.project.projectModel.MatModel.PaperType;
          if (0 == t) {
            let A = Number.parseInt(e[2] + (e[3] << 8));
            this.pageObject.DPI = Number.parseFloat(A / 100);
          } else if (6 == t) {
            let A = Number.parseInt(e[0] + (e[1] << 8));
            this.pageObject.DPI = Number.parseFloat(A / 100);
          } else if (7 == t) {
            let A = Number.parseInt(e[4] + (e[5] << 8));
            this.pageObject.DPI = Number.parseFloat(A / 100);
          } else 8 == t && (this.pageObject.DPI = 11.606);
          (this.pageObject.DPI < 11 || this.pageObject.DPI > 12.8) && (this.pageObject.DPI = 11.8), a["a"].commit("project/editCurrentPrintPrintDPI", {
            DPI: this.pageObject.DPI
          }), this.closePrint();
        } else a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "状态查询命令失败!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
    }
  },
  printEnd() {
    this.closePrint();
  },
  closePrint() {
    g["a"].hidDeviceClose().then(A => {
      this.hidDevice = A, console.log("设备已关闭", this.hidDevice), a["a"].state.project.currentPrint.isPrintEnd = !0, a["a"].state.project.currentPrint.close = !0;
    });
  },
  waitNewPrint() {
    if (i["a"].pPageCnt > a["a"].state.project.currentPrint.num - a["a"].state.project.currentPrint.keepOnPrintPosition && this.objectLength > a["a"].state.project.currentPrint.num && a["a"].commit("project/currentPrintNumAdd"), a["a"].state.project.currentPrint.state) {
      if (!this.devCheckErrMsg()) return i["a"].fStaReg.PrtSta ? void (this.waitImageNum <= 0 || (a["a"].state.project.currentPrint.imageDataListAll.length > 0 ? (this.imageDataList = [], this.imageDataList = a["a"].state.project.currentPrint.imageDataListAll.shift(), setTimeout(() => {
        this.checkPrtStaNext();
      }, 100)) : setTimeout(() => {
        console.log("等待新的打印", this.waitImageNum), this.waitImageList(), this.waitImageNum--;
      }, 500))) : (this.step = 12, this.mCurrentCommand = this.CMD_INQUIRY_STA, void this.sendCmd(this.CMD_INQUIRY_STA, 0));
      this.closePrint();
    }
  },
  waitImageList() {
    this.step = 11, this.mCurrentCommand = this.CMD_INQUIRY_STA, this.sendCmd(this.CMD_INQUIRY_STA, 0);
  },
  handleTransferNext() {
    console.log(i["a"].pPageCnt, a["a"].state.project.currentPrint.num, a["a"].state.project.currentPrint.keepOnPrintPosition), i["a"].pPageCnt > a["a"].state.project.currentPrint.num - a["a"].state.project.currentPrint.keepOnPrintPosition && this.objectLength > a["a"].state.project.currentPrint.num && a["a"].commit("project/currentPrintNumAdd"), a["a"].state.project.currentPrint.state && (this.devCheckErrMsg() ? this.closePrint() : i["a"].mStaReg.BufFull ? setTimeout(() => {
      this.step = 10, console.log("等待上一张"), this.mCurrentCommand = this.CMD_INQUIRY_STA, this.sendCmd(this.mCurrentCommand, 0);
    }, 500) : setTimeout(() => {
      this.step = 6, this.handleStep();
    }, 100));
  },
  cmdbuffull() {
    console.log("进入置满", a["a"].state.project.currentPrint.sendCmdState), this.cmdBufFullNum > 50 ? console.log("设备超时，置满失败") : this.devCheckErrMsg() ? this.closePrint() : !a["a"].state.project.currentPrint.sendCmdState && a["a"].state.project.currentPrint.sendCmdMatrix.num == a["a"].state.project.currentPrint.sendCmdMatrix.total && a["a"].state.project.currentPrint.sendCmdMatrix.total > 0 ? setTimeout(() => {
      this.cmdBufFullNum = 0, a["a"].state.project.currentPrint.sendCmdMatrix.num = 0, a["a"].state.project.currentPrint.sendCmdMatrix.total = 0, this.n = 0, this.mCurrentCommand = this.CMD_BUF_FULL, this.sendCmd(this.CMD_BUF_FULL, 0);
    }, 10) : (console.log("重试置满"), setTimeout(() => {
      this.cmdBufFullNum++, this.cmdbuffull();
    }, 10));
  },
  sendMatrix() {
    this.sedMatrixNum > 50 ? console.log("设备超时，发送字模失败") : this.devCheckErrMsg() ? this.closePrint() : a["a"].state.project.currentPrint.sendCmdState ? (console.log("重试sedMatrix"), setTimeout(() => {
      this.sedMatrixNum++, this.sendMatrix();
    }, 200)) : setTimeout(() => {
      this.sedMatrixNum = 0, g["a"].BulkWriteType(this.imageData, this.imageData.length, !1), setTimeout(() => {
        this.step = 9, this.handleStep();
      }, 100);
    }, 10);
  },
  handleTransferStep() {
    const A = this;
    if (!A.devCheckErrMsg()) return A.sendDataNum < 0 ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "状态查询命令失败"
    }), A.step = 0, A.mCurrentCommand = 0, A.sendDataNum = 50, void A.closePrint()) : void (A.mCurrentCommand == A.CMD_INQUIRY_STA && 0 == i["a"].mStaReg.BufFull ? (A.sendDataNum = 50, A.imageData = A.imageDataList.shift(), A.imageDataList.length <= 0 && (console.log("渲染下一张"), a["a"].state.project.currentPrint.nextPrintState = !0), A.mCurrentCommand = A.CMD_NEXTFRM_BULK, A.sendCmd(A.CMD_NEXTFRM_BULK, A.imageData.length)) : setTimeout(() => {
      A.sendDataNum--, A.step = 7, A.mCurrentCommand = A.CMD_INQUIRY_STA, A.sendCmd(A.mCurrentCommand, 0);
    }, 2));
    A.closePrint();
  },
  spPrintImage() {
    this.tpPrintImage();
  },
  tpPrintImage() {
    this.step = 7, this.mCurrentCommand = this.CMD_INQUIRY_STA, this.sendCmd(this.CMD_INQUIRY_STA, 0);
  },
  checkPrtStaNext() {
    const A = this;
    setTimeout(function () {
      A.step = 10, A.mCurrentCommand = A.CMD_INQUIRY_STA, A.sendCmd(A.mCurrentCommand, 0);
    }, 20);
  },
  checkPrtSta() {
    const A = this;
    return console.log("启动检测", i["a"].mStaReg.ComExeSta), A.mCurrentCommand == A.CMD_INQUIRY_STA && i["a"].fStaReg.PrtSta ? (A.checkPrtStaNum = 20, A.speed = Math.min(a["a"].state.project.currentPrint.speed, 60), A.speed = Math.max(a["a"].state.project.currentPrint.speed, 20), this.step = 6, void this.handleStep()) : A.checkPrtStaNum < 0 ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "启动打印失败"
    }), A.step = 0, A.mCurrentCommand = 0, void A.closePrint()) : void setTimeout(function () {
      A.checkPrtStaNum--, A.mCurrentCommand = A.CMD_INQUIRY_STA, A.sendCmd(A.mCurrentCommand, 0);
    }, 500);
  },
  startTP() {
    this.devCheckErrMsg() ? this.closePrint() : (console.log("启动打印"), this.ma = a["a"].state.project.currentPrint.deepness << 8, this.mCurrentCommand = this.CMD_STATR_PRINT, this.sendCmd(this.mCurrentCommand, this.ma));
  },
  waitComOk() {
    const A = this;
    return A.mCurrentCommand == A.CMD_INQUIRY_STA && 0 == i["a"].mStaReg.ComExeSta ? (A.waitComOkNum = 30, A.step = 3, void A.handleStep()) : A.waitComOkNum < 0 ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "设备检测命令超时"
    }), A.step = 0, A.mCurrentCommand = 0, void A.closePrint()) : void setTimeout(function () {
      A.waitComOkNum--, A.mCurrentCommand = A.CMD_INQUIRY_STA, A.sendCmd(A.mCurrentCommand, 0);
    }, 500);
  },
  handleInquiryStatus(A) {
    let e = new Uint8Array(8);
    return e[0] = A[1], e[1] = A[2], e[2] = A[3], e[3] = A[4], e[4] = A[5], e[5] = A[6], e[6] = A[7], e[7] = A[8], i["a"].fStaRegFromBytes(e, 2), i["a"].mStaRegFromBytes(e, 0), i["a"].pPageCnt = e[5], i["a"].pPageCnt <<= 8, i["a"].pPageCnt += e[4], i["a"].SendData = e[7], i["a"].SendData <<= 8, i["a"].SendData += e[6], !0;
  },
  devCheckErrMsg() {
    return i["a"].fStaReg.CoverOpen ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "上盖打开"
    }), !0) : i["a"].fStaReg.RibEnd ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "光电检测不到色带"
    }), !0) : i["a"].fStaReg.LabEnd ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "贴纸用完"
    }), !0) : i["a"].fStaReg.LabRwErr ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "贴纸读取错误"
    }), !0) : i["a"].fStaReg.qCutErr ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "切刀出错"
    }), !0) : i["a"].mStaReg.RibRdErr ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "色带出错"
    }), !0) : i["a"].mStaReg.MatIn ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "入口检测不到材料"
    }), !0) : i["a"].mStaReg.LabRdErr ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "贴纸读取错误"
    }), !0) : i["a"].mStaReg.RibBreak ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "色带断带"
    }), !0) : i["a"].mStaReg.RibOver ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "色带用完"
    }), !0) : i["a"].mStaReg.LabOver ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "贴纸用完"
    }), !0) : !!i["a"].fStaReg.NeedAuthen && (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "请联网后，重新开启打印设备"
    }), !0);
  },
  getDpi() {
    if (a["a"].state.project.projectModel && a["a"].state.project.projectModel.MatModel) return a["a"].state.project.currentPrint.isStop = !1, this.mCurrentCommand = this.CMD_RD_LAB_DPI, void g["a"].SendCmd(this.mCurrentCommand, 0);
  }
}

// --- CMD_CONSTANTS_OBJECT [ObjectExpression with CMD constants] (line 3, col 3096260) ---
{
  CMD_BUF_FULL: 16,
  CMD_INQUIRY_STA: 17,
  CMD_STATR_PRINT: 19,
  CMD_FINISH_PRINT: 17,
  CMD_STOP_PRINT: 20,
  CMD_CHECK_DEVICE: 18,
  CMD_CHECK_RIB: 25,
  CMD_RESET_PRINT: 20,
  CMD_RD_LAB_DPI: 34,
  CMD_NEXTFRM_BULK: 92,
  imageDataList: [],
  isStop: !1,
  mCurrentCommand: 0,
  step: 0,
  waitComOkNum: 20,
  maxDotValue: 0,
  checkPrtStaNum: 50,
  pageObject: {},
  sedMatrixNum: 0,
  cmdBufFullNum: 0,
  speed: 60,
  waitImageNum: 0,
  objectLength: 0,
  imageData: [],
  waitStopPrintNum: 0,
  ma: 0,
  startPrint(A, e) {
    this.isStop = !1, this.waitComOkNum = 20, this.checkPrtStaNum = 20, this.sedMatrixNum = 0, this.cmdBufFullNum = 0, this.waitImageNum = 0, this.imageDataList = a["a"].state.project.currentPrint.imageDataListAll.shift(), this.objectLength = e, this.pageObject = A, this.step = 1, this.handleStep();
  },
  sendCmd(A, e) {
    if (!this.isStop) return a["a"].state.project.currentPrint.isStop ? (this.isStop = !0, void this.stopPrintManual()) : void (a["a"].state.project.currentPrint.sendCmdState || g["a"].SendCmd(A, e));
  },
  stopPrintManual() {
    const A = this;
    A.waitStopPrintNum > 50 ? console.log("设备超时，终止失败") : a["a"].state.project.currentPrint.sendCmdState ? setTimeout(() => {
      A.waitStopPrintNum++, A.waitStopPrintManual(), console.log("设备忙碌中，等待设备空闲在发送命令");
    }, 500) : i["a"].fStaReg.PrtSta ? (g["a"].SendCmd(A.CMD_STOP_PRINT, 0), console.log("发送终止命令")) : this.closePrint();
  },
  waitStopPrintManual() {
    const A = this;
    A.devCheckErrMsg() ? this.closePrint() : i["a"].fStaReg.PrtSta ? a["a"].state.project.currentPrint.sendCmdState ? (A.waitStopPrintManual(), console.log("设备忙碌中，等待设备空闲在发送命令")) : setTimeout(() => {
      g["a"].SendCmd(A.CMD_INQUIRY_STA, 0);
    }, 500) : this.closePrint();
  },
  handleNotifyStop(A) {
    this.handleInquiryStatus(A) && this.waitStopPrintManual();
  },
  handleStep() {
    const A = this;
    switch (A.step) {
      case 0:
        break;
      case 1:
        A.ma = Number.parseInt(I["a"].getTPMaterialTypeCode() << 8) + Number.parseInt(this.pageObject.CutType), A.mCurrentCommand = A.CMD_CHECK_DEVICE, A.sendCmd(A.mCurrentCommand, A.ma);
        break;
      case 2:
        A.waitComOk();
        break;
      case 3:
        if (A.devCheckErrMsg()) return void this.closePrint();
        this.step = 4, this.handleStep();
        break;
      case 4:
        A.startTP();
        break;
      case 5:
        A.checkPrtSta();
        break;
      case 6:
        A.handleTransferStep();
        break;
      case 7:
        A.sendMatrix();
        break;
      case 8:
        A.cmdbuffull();
        break;
      case 9:
        A.handleTransferNext();
        break;
      case 10:
        A.waitNewPrint();
        break;
      case 11:
        A.printEnd();
        break;
    }
  },
  handleNotify(A) {
    switch (this.mCurrentCommand) {
      case this.CMD_INQUIRY_STA:
        if (this.handleInquiryStatus(A)) return void this.handleStep();
        a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "状态查询命令失败!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
      case this.CMD_CHECK_DEVICE:
        if (this.handleInquiryStatus(A)) return this.step = 2, void this.handleStep();
        a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "检测设备失败!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
      case this.CMD_STATR_PRINT:
        this.handleInquiryStatus(A) ? (this.step = 5, this.handleStep()) : (a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "启动打印失败"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint());
        break;
      case this.CMD_NEXTFRM_BULK:
        this.handleInquiryStatus(A) ? (this.step = 7, this.handleStep()) : (a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "传输数据错误!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint());
        break;
      case this.CMD_BUF_FULL:
        console.log("******************接收到志满完成命令回复*****************************"), a["a"].state.project.currentPrint.imageDataListAll.length > 0 ? (this.imageDataList = [], this.imageDataList = a["a"].state.project.currentPrint.imageDataListAll.shift(), setTimeout(() => {
          this.checkPrtStaNext();
        }, 100)) : setTimeout(() => {
          this.waitImageNum = 300, this.waitImageList();
        }, 100);
        break;
      case this.CMD_RD_LAB_DPI:
        if (this.handleInquiryStatus(A)) {
          let e = new Uint8Array(8);
          e[0] = A[1], e[1] = A[2], e[2] = A[3], e[3] = A[4], e[4] = A[5], e[5] = A[6], e[6] = A[7], e[7] = A[8];
          let t = a["a"].state.project.projectModel.MatModel.PaperType;
          if (0 == t) {
            let A = Number.parseInt(e[2] + (e[3] << 8));
            this.pageObject.DPI = Number.parseFloat(A / 100);
          } else if (6 == t) {
            let A = Number.parseInt(e[0] + (e[1] << 8));
            this.pageObject.DPI = Number.parseFloat(A / 100);
          } else if (7 == t) {
            let A = Number.parseInt(e[4] + (e[5] << 8));
            this.pageObject.DPI = Number.parseFloat(A / 100);
          } else 8 == t && (this.pageObject.DPI = 11.606);
          (this.pageObject.DPI < 11 || this.pageObject.DPI > 12.8) && (this.pageObject.DPI = 11.8), a["a"].commit("project/editCurrentPrintPrintDPI", {
            DPI: this.pageObject.DPI
          }), this.closePrint();
        } else a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "状态查询命令失败!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
    }
  },
  printEnd() {
    this.closePrint();
  },
  closePrint() {
    g["a"].hidDeviceClose().then(A => {
      this.hidDevice = A, console.log("设备已关闭", this.hidDevice), a["a"].state.project.currentPrint.isPrintEnd = !0, a["a"].state.project.currentPrint.close = !0;
    });
  },
  waitNewPrint() {
    if (i["a"].pPageCnt > a["a"].state.project.currentPrint.num - a["a"].state.project.currentPrint.keepOnPrintPosition && this.objectLength > a["a"].state.project.currentPrint.num && a["a"].commit("project/currentPrintNumAdd"), a["a"].state.project.currentPrint.state) {
      if (!this.devCheckErrMsg()) return i["a"].fStaReg.PrtSta ? void (this.waitImageNum <= 0 || (a["a"].state.project.currentPrint.imageDataListAll.length > 0 ? (this.imageDataList = [], this.imageDataList = a["a"].state.project.currentPrint.imageDataListAll.shift(), setTimeout(() => {
        this.checkPrtStaNext();
      }, 100)) : setTimeout(() => {
        console.log("等待新的打印", this.waitImageNum), this.waitImageList(), this.waitImageNum--;
      }, 500))) : (this.step = 11, this.mCurrentCommand = this.CMD_INQUIRY_STA, void this.sendCmd(this.CMD_INQUIRY_STA, 0));
      this.closePrint();
    }
  },
  waitImageList() {
    this.step = 10, this.mCurrentCommand = this.CMD_INQUIRY_STA, this.sendCmd(this.CMD_INQUIRY_STA, 0);
  },
  handleTransferNext() {
    console.log(i["a"].pPageCnt, a["a"].state.project.currentPrint.num, a["a"].state.project.currentPrint.keepOnPrintPosition), i["a"].pPageCnt > a["a"].state.project.currentPrint.num - a["a"].state.project.currentPrint.keepOnPrintPosition && this.objectLength > a["a"].state.project.currentPrint.num && a["a"].commit("project/currentPrintNumAdd"), a["a"].state.project.currentPrint.state && (this.devCheckErrMsg() ? this.closePrint() : i["a"].mStaReg.BufFull ? setTimeout(() => {
      this.step = 9, console.log("等待上一张"), this.mCurrentCommand = this.CMD_INQUIRY_STA, this.sendCmd(this.mCurrentCommand, 0);
    }, 500) : setTimeout(() => {
      this.step = 6, this.handleStep();
    }, 100));
  },
  cmdbuffull() {
    console.log("进入置满", a["a"].state.project.currentPrint.sendCmdState), this.cmdBufFullNum > 50 ? console.log("设备超时，置满失败") : this.devCheckErrMsg() ? this.closePrint() : !a["a"].state.project.currentPrint.sendCmdState && a["a"].state.project.currentPrint.sendCmdMatrix.num == a["a"].state.project.currentPrint.sendCmdMatrix.total && a["a"].state.project.currentPrint.sendCmdMatrix.total > 0 ? setTimeout(() => {
      this.cmdBufFullNum = 0, a["a"].state.project.currentPrint.sendCmdMatrix.num = 0, a["a"].state.project.currentPrint.sendCmdMatrix.total = 0, this.n = 0, this.mCurrentCommand = this.CMD_BUF_FULL, this.sendCmd(this.CMD_BUF_FULL, this.imageDataList.length);
    }, 100) : (console.log("重试置满"), setTimeout(() => {
      this.cmdBufFullNum++, this.cmdbuffull();
    }, 200));
  },
  sendMatrix() {
    this.sedMatrixNum > 50 ? console.log("设备超时，发送字模失败") : this.devCheckErrMsg() ? this.closePrint() : a["a"].state.project.currentPrint.sendCmdState ? (console.log("重试sedMatrix"), setTimeout(() => {
      this.sedMatrixNum++, this.sendMatrix();
    }, 200)) : setTimeout(() => {
      this.sedMatrixNum = 0, g["a"].BulkWriteType(this.imageDataList, this.imageDataList.length, !1), setTimeout(() => {
        this.step = 8, this.handleStep();
      }, 50);
    }, 100);
  },
  handleTransferStep() {
    this.devCheckErrMsg() ? this.closePrint() : (a["a"].state.project.currentPrint.nextPrintState = !0, this.mCurrentCommand = this.CMD_NEXTFRM_BULK, this.sendCmd(this.CMD_NEXTFRM_BULK, this.imageDataList.length));
  },
  checkPrtStaNext() {
    const A = this;
    setTimeout(function () {
      A.step = 9, A.mCurrentCommand = A.CMD_INQUIRY_STA, A.sendCmd(A.mCurrentCommand, 0);
    }, 20);
  },
  checkPrtSta() {
    const A = this;
    return console.log("启动检测", i["a"].mStaReg.ComExeSta), A.mCurrentCommand == A.CMD_INQUIRY_STA && 0 == i["a"].mStaReg.ComExeSta ? (A.checkPrtStaNum = 20, A.speed = Math.min(a["a"].state.project.currentPrint.speed, 60), A.speed = Math.max(a["a"].state.project.currentPrint.speed, 20), this.step = 6, void this.handleStep()) : A.checkPrtStaNum < 0 ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "启动打印失败"
    }), A.step = 0, A.mCurrentCommand = 0, void A.closePrint()) : void setTimeout(function () {
      A.checkPrtStaNum--, A.mCurrentCommand = A.CMD_INQUIRY_STA, A.sendCmd(A.mCurrentCommand, 0);
    }, 500);
  },
  startTP() {
    this.devCheckErrMsg() ? this.closePrint() : (console.log("启动打印"), this.ma = a["a"].state.project.currentPrint.deepness << 8, this.mCurrentCommand = this.CMD_STATR_PRINT, this.sendCmd(this.mCurrentCommand, this.ma));
  },
  waitComOk() {
    const A = this;
    return A.mCurrentCommand == A.CMD_INQUIRY_STA && 0 == i["a"].mStaReg.ComExeSta ? (A.waitComOkNum = 20, A.step = 3, void A.handleStep()) : A.waitComOkNum < 0 ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "设备检测命令超时"
    }), A.step = 0, A.mCurrentCommand = 0, void A.closePrint()) : void setTimeout(function () {
      A.waitComOkNum--, A.mCurrentCommand = A.CMD_INQUIRY_STA, A.sendCmd(A.mCurrentCommand, 0);
    }, 500);
  },
  handleInquiryStatus(A) {
    let e = new Uint8Array(8);
    return e[0] = A[1], e[1] = A[2], e[2] = A[3], e[3] = A[4], e[4] = A[5], e[5] = A[6], e[6] = A[7], e[7] = A[8], i["a"].fStaRegFromBytes(e, 2), i["a"].mStaRegFromBytes(e, 0), i["a"].pPageCnt = e[5], i["a"].pPageCnt <<= 8, i["a"].pPageCnt += e[4], i["a"].SendData = e[7], i["a"].SendData <<= 8, i["a"].SendData += e[6], !0;
  },
  devCheckErrMsg() {
    return i["a"].fStaReg.CoverOpen ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "上盖打开"
    }), !0) : i["a"].fStaReg.RibEnd ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "光电检测不到色带"
    }), !0) : i["a"].fStaReg.LabEnd ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "贴纸用完"
    }), !0) : i["a"].fStaReg.LabRwErr ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "贴纸读取错误"
    }), !0) : i["a"].fStaReg.qCutErr ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "切刀出错"
    }), !0) : i["a"].mStaReg.RibRdErr ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "色带出错"
    }), !0) : i["a"].mStaReg.MatIn ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "入口检测不到材料"
    }), !0) : i["a"].mStaReg.LabRdErr ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "贴纸读取错误"
    }), !0) : i["a"].mStaReg.RibBreak ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "色带断带"
    }), !0) : i["a"].mStaReg.RibOver ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "色带用完"
    }), !0) : i["a"].mStaReg.LabOver ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "贴纸用完"
    }), !0) : !!i["a"].fStaReg.NeedAuthen && (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "请联网后，重新开启打印设备"
    }), !0);
  },
  getDpi() {
    if (a["a"].state.project.projectModel && a["a"].state.project.projectModel.MatModel) return a["a"].state.project.currentPrint.isStop = !1, this.mCurrentCommand = this.CMD_RD_LAB_DPI, void g["a"].SendCmd(this.mCurrentCommand, 0);
  }
}

// --- CMD_CONSTANTS_OBJECT [ObjectExpression with CMD constants] (line 3, col 3297251) ---
{
  CMD_BUF_FULL: 16,
  CMD_INQUIRY_STA: 17,
  CMD_CHECK_DEVICE: 18,
  CMD_STATR_PRINT: 19,
  CMD_NEXTFRM_FIRMWARE_BULK: 198,
  CMD_STOP_PRINT: 20,
  CMD_RETURN_MAT: 48,
  CMD_RETURN_DEVICE: 8888,
  CMD_NEXTFRM_BULK: 92,
  CMD_READ_FIRMWARE_REV: 197,
  CMD_TRANSFER: 240,
  CMD_TRANSFER_ONE: 888,
  CMD_SET_RFID_DATA: 93,
  CMD_SET_RFID_DATA_WRITE: 999,
  CMD_READ_REV: 23,
  ERROR_CMD_INQUIRY_STA: 123,
  dataLength: 500,
  nCnt: 0,
  step: 0,
  n: 0,
  mCurrentCommand: 0,
  isTransferPart: !1,
  partIndex: 0,
  count: 0,
  byteArrayList: [],
  imageDataListAll: [],
  imageDataList: [],
  isStop: !1,
  RfidData: [],
  printerSn: "",
  paperType: 1,
  gap: 8,
  speed: 60,
  noInstallType: !1,
  isNetWork: !1,
  copiesAll: 0,
  mMatWidth: 0,
  mMatHeiht: 0,
  isPrintFlow: 0,
  statusNum: 0,
  errorMessage: "",
  waitImageNum: 0,
  waitStopPrintNum: 0,
  waitEndPrintNum: 0,
  objectLength: 0,
  sendCmdState: !1,
  sendCmdNum: 0,
  waitSendCmdNum: 0,
  sedMatrixNum: 0,
  cmdBufFullNum: 0,
  sendRfidDataNum: 0,
  waitComOkNum: 20,
  checkStopPrintNum: 20,
  divRfidData: [],
  imageData: [],
  sendDataNum: 200,
  setWidthAndHeight(A, e, t) {
    const g = this;
    g.mMatWidth = A, g.mMatHeiht = e, g.copiesAll = t;
  },
  doSupVanPrint(A, e, t, g, i, I, B, E, C) {
    const n = this;
    this.isStop = !1, n.statusNum = 100, this.waitImageNum = 0, this.imageDataList = a["a"].state.project.currentPrint.imageDataListAll.shift(), this.step = 1, this.RfidData = [], this.printerSn = t, this.paperType = g, this.gap = i, this.speed = I, this.isNetWork = B, this.isPrintFlow = E, this.objectLength = C, n.sendCmdNum = 0, n.cmdBufFullNum = 0, n.waitComOkNum = 20, n.checkStopPrintNum = 20, n.sedMatrixNum = 0, n.divRfidData = [], this.sendDataNum = 200, n.RfidData = [48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 121, 1, 1, 91, 235, 93, 155, 179, 48, 117, 1, 50, 50, 1, 3, 0, 224, 1, 0, 0, 164, 6, 176, 4, 23, 8, 17, 11, 48, 57, 0, 0, 135, 220, 151, 205, 1, 224, 159, 64, 149, 68, 77, 133, 236, 167, 205, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], B && (n.RfidData = [], n.RfidData = e), n.RfidData = e, this.imageDataList.length > 1 ? this.speed = 20 : this.speed = 60, this.startPrint();
  },
  startPrint() {
    const A = this;
    this.waitStopPrintNum = 0, this.waitEndPrintNum = 0, 0 == this.RfidData.length ? (A.step = 1, A.handleStep()) : (A.step = 20, A.handleStep());
  },
  sendCmd(A, e) {
    if (!this.isStop) return a["a"].state.project.currentPrint.isStop ? (this.isStop = !0, this.step = 24, void this.handleStep()) : void (a["a"].state.project.currentPrint.sendCmdState || g["a"].SendCmd(A, e));
  },
  handleStep() {
    const A = this;
    switch (A.step) {
      case 0:
        break;
      case 1:
        A.mCurrentCommand = A.CMD_CHECK_DEVICE, A.sendCmd(A.mCurrentCommand, 0);
        break;
      case 2:
        A.waitComOk();
        break;
      case 3:
        if (A.devCheckErrMsg()) return void this.closePrint();
        if (i["a"].printingStation) return void a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "打印中，稍后再试"
        });
        A.startT5080();
        break;
      case 4:
        if (A.devCheckErrMsg()) return void this.closePrint();
        A.handleTransferStep();
        break;
      case 5:
        if (A.devCheckErrMsg()) return void this.closePrint();
        this.sendMatrix();
        break;
      case 6:
        if (A.devCheckErrMsg()) return void this.closePrint();
        this.cmdbuffull();
        break;
      case 7:
        if (A.devCheckErrMsg()) return void this.closePrint();
        A.handleTransferNext();
        break;
      case 8:
        if (A.devCheckErrMsg()) return void this.closePrint();
        A.waitNewPrint();
        break;
      case 9:
        A.printEnd();
        break;
      case 10:
        A.waitClose();
        break;
      case 11:
        A.closePrint();
        break;
      case 20:
        A.setRfidData();
        break;
      case 21:
        A.sendRfidData();
        break;
      case 22:
        A.sendDivRfidData();
        break;
      case 23:
        a["a"].state.mat.sendRfidState = !0, this.closePrint();
        break;
      case 24:
        this.checkStopPrint();
        break;
      case 25:
        this.stopPrintManual();
        break;
    }
  },
  handleNotify(A) {
    switch (this.mCurrentCommand) {
      case this.CMD_CHECK_DEVICE:
        if (g["a"].handleInquiryStatus(A)) return this.step = 2, void this.handleStep();
        a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "检测设备失败!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
      case this.CMD_INQUIRY_STA:
        if (g["a"].handleInquiryStatus(A)) return void this.handleStep();
        a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "状态查询命令失败!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
      case this.CMD_STATR_PRINT:
        if (g["a"].handleInquiryStatus(A)) return this.step = 4, void this.handleStep();
        a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "启动打印失败"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
      case this.CMD_NEXTFRM_BULK:
        if (g["a"].handleInquiryStatus(A)) return this.step = 5, void this.handleStep();
        a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "传输数据错误!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
      case this.CMD_TRANSFER_ONE:
        if (g["a"].handleInquiryStatus(A)) return this.step = 6, void this.handleStep();
        a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "传输数据错误!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
      case this.CMD_BUF_FULL:
        if (console.log("******************接收到志满完成命令回复*****************************"), g["a"].handleInquiryStatus(A)) {
          this.imageDataList != [] && this.imageDataList.length > 0 ? setTimeout(() => {
            this.sendDataNum = 200, this.step = 4, this.handleStep();
          }, 100) : 0 == this.isPrintFlow && (a["a"].state.project.currentPrint.imageDataListAll.length > 0 ? (this.imageDataList = [], this.imageDataList = a["a"].state.project.currentPrint.imageDataListAll.shift(), this.imageDataList.length > 1 ? this.speed = 20 : this.speed = 60, setTimeout(() => {
            this.handleTransferNext();
          }, 100)) : setTimeout(() => {
            this.waitImageNum = 20, this.waitImageList();
          }, 100));
          break;
        }
        a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "传输数据错误!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
      case this.CMD_SET_RFID_DATA:
        if (g["a"].handleInquiryStatus(A)) return 22 != this.step && (this.step = 21), void this.handleStep();
        break;
      case this.CMD_SET_RFID_DATA_WRITE:
        g["a"].handleInquiryStatus(A) && (23 != this.step && (this.step = 1), this.handleStep());
        break;
      case this.CMD_STOP_PRINT:
        if (g["a"].handleInquiryStatus(A)) return this.step = 10, void this.handleStep();
        a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "状态查询命令失败!"
        }), this.step = 0, this.mCurrentCommand = 0, this.closePrint();
        break;
      case this.CMD_RETURN_MAT:
        if (g["a"].handleInquiryStatus(A)) {
          let e = Number.parseInt(A[16] + ((255 & A[17]) << 8)),
            t = this.byteToString(A, 40, 16),
            g = this.bytesToString(A, 1, 7);
          g && (g = this.padRight(g, 14, "0"));
          let i = A[21],
            B = A[18],
            C = Object(E["a"])(I["a"]);
          C.Sn = e, C.DeviceSn = t, C.UUID = g, C.Gap = i, C.PaperType = B, C.UUID.endsWith("000000") ? a["a"].commit("shareMemery/editIsCustomLable", {
            isCustomLable: !0
          }) : a["a"].commit("shareMemery/editIsCustomLable", {
            isCustomLable: !1
          }), a["a"].commit("mat/setAutoIdentifyMat", {
            autoIdentifyMat: C
          });
        } else this.step = 0, this.mCurrentCommand = 0;
        this.closePrint();
        break;
      case this.CMD_RETURN_DEVICE:
        if (g["a"].handleInquiryStatus(A)) {
          let e = Number.parseInt(A[16] + ((255 & A[17]) << 8)),
            t = this.byteToString(A, 40, 16),
            g = this.bytesToString(A, 1, 7);
          g && (g = this.padRight(g, 14, "0"));
          let i = A[21],
            I = A[18],
            C = Object(E["a"])(B["a"]);
          C.Sn = e, C.DeviceSn = t, C.UUID = g, C.Gap = i, C.PaperType = I, a["a"].commit("shareMemery/setPCUserAction", {
            pcUserAction: C
          });
        } else this.step = 0, this.mCurrentCommand = 0;
        this.closePrint();
        break;
    }
  },
  waitClose() {
    console.log(i["a"].printingStation, this.waitEndPrintNum), i["a"].printingStation && this.waitEndPrintNum < 20 ? setTimeout(() => {
      this.step = 10, this.mCurrentCommand = this.CMD_INQUIRY_STA, g["a"].SendCmd(this.mCurrentCommand, 0), this.waitEndPrintNum++;
    }, 200) : (this.mCurrentCommand = 0, this.closePrint());
  },
  printEnd() {
    this.closePrint();
  },
  waitNewPrint() {
    if (console.log(i["a"].pPageCnt, this.objectLength - 1, i["a"].printingStation), i["a"].pPageCnt > a["a"].state.project.currentPrint.num - a["a"].state.project.currentPrint.keepOnPrintPosition && this.objectLength > a["a"].state.project.currentPrint.num && a["a"].commit("project/currentPrintNumAdd"), i["a"].pPageCnt >= this.objectLength - 1 || 1 == this.objectLength || !i["a"].printingStation) return console.log("关闭"), this.objectLength > a["a"].state.project.currentPrint.num && a["a"].commit("project/currentPrintNumAdd"), this.step = 9, this.mCurrentCommand = this.CMD_INQUIRY_STA, void this.sendCmd(this.CMD_INQUIRY_STA, 0);
    this.waitImageNum <= 0 || (a["a"].state.project.currentPrint.imageDataListAll.length > 0 ? (this.imageDataList = [], this.imageDataList = a["a"].state.project.currentPrint.imageDataListAll.shift(), this.imageDataList.length > 1 ? this.speed = 20 : this.speed = 60, setTimeout(() => {
      this.handleTransferNext();
    }, 100)) : setTimeout(() => {
      console.log("等待新的打印", this.waitImageNum), this.waitImageList(), this.waitImageNum--;
    }, 500));
  },
  waitImageList() {
    this.step = 8, this.mCurrentCommand = this.CMD_INQUIRY_STA, this.sendCmd(this.CMD_INQUIRY_STA, 0);
  },
  setRfidData() {
    this.mCurrentCommand = this.CMD_SET_RFID_DATA, this.sendCmd(this.mCurrentCommand, this.RfidData.length);
  },
  sendRfidData() {
    this.mCurrentCommand = this.CMD_SET_RFID_DATA_WRITE, g["a"].BulkWrite(this.RfidData, this.RfidData.length);
  },
  setDivRfidData(A) {
    this.divRfidData = A, this.step = 22, this.mCurrentCommand = this.CMD_SET_RFID_DATA, this.sendCmd(this.mCurrentCommand, this.divRfidData.length);
  },
  sendDivRfidData() {
    this.step = 23, this.mCurrentCommand = this.CMD_SET_RFID_DATA_WRITE, g["a"].BulkWrite(this.divRfidData, this.divRfidData.length);
  },
  handleTransferNext() {
    if (console.log(i["a"].BufFull), i["a"].pPageCnt > a["a"].state.project.currentPrint.num - a["a"].state.project.currentPrint.keepOnPrintPosition && this.objectLength > a["a"].state.project.currentPrint.num && a["a"].commit("project/currentPrintNumAdd"), a["a"].state.project.currentPrint.state) {
      if (this.devCheckErrMsg()) this.closePrint();else if (i["a"].BufFull) {
        if (console.log("等待上一张打印完成"), !i["a"].printingStation) return a["a"].commit("project/editCurrentPrintState", {
          state: !1,
          errorMsg: "打印被终止!"
        }), void this.closePrint();
        setTimeout(() => {
          this.step = 7, this.mCurrentCommand = this.CMD_INQUIRY_STA, this.sendCmd(this.mCurrentCommand, 0);
        }, 50);
      } else console.log("打印下一张"), setTimeout(() => {
        this.step = 4, this.handleStep();
      }, 100);
    } else this.closePrint();
  },
  cmdbuffull() {
    console.log("进入置满", a["a"].state.project.currentPrint.sendCmdState), this.cmdBufFullNum > 50 ? console.log("设备超时，置满失败") : a["a"].state.project.currentPrint.sendCmdState || a["a"].state.project.currentPrint.sendCmdMatrix.num != a["a"].state.project.currentPrint.sendCmdMatrix.total ? (console.log("重试置满"), setTimeout(() => {
      this.cmdBufFullNum++, this.cmdbuffull();
    }, 100)) : setTimeout(() => {
      this.cmdBufFullNum = 0, a["a"].state.project.currentPrint.sendCmdMatrix.num = 0, a["a"].state.project.currentPrint.sendCmdMatrix.total = 0, this.n = 0, this.mCurrentCommand = this.CMD_BUF_FULL;
      let A = this.speed;
      console.log(A, "speed"), g["a"].SendCmdTwo(this.mCurrentCommand, this.imageData.length, A);
    }, 100);
  },
  sendMatrix() {
    this.sedMatrixNum > 50 ? console.log("设备超时，发送字模失败") : a["a"].state.project.currentPrint.sendCmdState ? (console.log("重试sedMatrix"), setTimeout(() => {
      this.sedMatrixNum++, this.sendMatrix();
    }, 200)) : setTimeout(() => {
      this.sedMatrixNum = 0, this.mCurrentCommand = this.CMD_TRANSFER_ONE, g["a"].BulkWriteType(this.imageData, this.imageData.length);
    }, 100);
  },
  handleTransferStep() {
    const A = this;
    if (!A.devCheckErrMsg()) return A.sendDataNum < 0 ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "状态查询命令失败"
    }), A.step = 0, A.mCurrentCommand = 0, A.sendDataNum = 200, void A.closePrint()) : void (A.mCurrentCommand == A.CMD_INQUIRY_STA && 0 == i["a"].BufFull ? (A.sendDataNum = 200, A.imageData = A.imageDataList.shift(), A.imageDataList.length <= 1 && (console.log("渲染下一张"), a["a"].state.project.currentPrint.nextPrintState = !0), this.mCurrentCommand = this.CMD_NEXTFRM_BULK, this.sendCmd(this.mCurrentCommand, A.imageData.length)) : (console.log("发送数据前检测"), setTimeout(() => {
      A.sendDataNum--, A.step = 4, A.mCurrentCommand = A.CMD_INQUIRY_STA, A.sendCmd(A.mCurrentCommand, 0);
    }, 5)));
    A.closePrint();
  },
  startT5080() {
    const A = this;
    A.mCurrentCommand = this.CMD_STATR_PRINT, this.sendCmd(this.mCurrentCommand, 1);
  },
  waitComOk() {
    const A = this;
    return A.mCurrentCommand == A.CMD_INQUIRY_STA && 0 == i["a"].deviceBusy ? (A.step = 3, void A.handleStep()) : A.waitComOkNum < 0 ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "检测设备超时!"
    }), A.step = 0, A.mCurrentCommand = 0, void A.closePrint()) : void setTimeout(function () {
      A.waitComOkNum--, A.mCurrentCommand = A.CMD_INQUIRY_STA, A.sendCmd(A.mCurrentCommand, 0);
    }, 500);
  },
  closePrint() {
    const A = this;
    i["a"].printingStation ? setTimeout(function () {
      this.step = 11, console.log("等待设备释放", i["a"].printingStation), A.mCurrentCommand = A.CMD_INQUIRY_STA, g["a"].SendCmd(A.mCurrentCommand, 0);
    }, 500) : g["a"].hidDeviceClose().then(A => {
      this.hidDevice = A, console.log("设备已关闭", this.hidDevice, i["a"].printingStation), a["a"].state.project.currentPrint.isPrintEnd = !0, a["a"].state.project.currentPrint.close = !0;
    });
  },
  devCheckErrMsg() {
    if (i["a"].loamCakeOpen) return a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "请关闭耗材仓盖"
    }), !0;
    if (i["a"].labelTECOError) return a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "耗材未装好"
    }), !0;
    if (i["a"].labelReadWriteError) return a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "未检测到耗材"
    }), !0;
    if (i["a"].labelModeError) a["a"].commit("project/editCurrentPrintState", {
      state: !0,
      errorMsg: "未识别到耗材"
    });else {
      if (i["a"].labelNoMore) return a["a"].commit("project/editCurrentPrintState", {
        state: !1,
        errorMsg: "耗材已用完"
      }), !0;
      if (i["a"].lowBattery) a["a"].commit("project/editCurrentPrintState", {
        state: !0,
        errorMsg: "电量低,请充电"
      });else if (i["a"].ChkMatflg) a["a"].commit("project/editCurrentPrintState", {
        state: !0,
        errorMsg: "请检查耗材余量"
      });else if (i["a"].headTempHigh) return a["a"].commit("project/editCurrentPrintState", {
        state: !1,
        errorMsg: "打印头温度过高"
      }), !0;
    }
    return !1;
  },
  checkStopPrint() {
    const A = this;
    return A.mCurrentCommand == A.CMD_INQUIRY_STA ? (console.log("checkStopPrint", i["a"].printingStation), i["a"].printingStation ? (A.step = 25, void A.handleStep()) : (A.step = 10, void A.handleStep())) : A.checkStopPrintNum < 0 ? (a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "检测设备超时!"
    }), A.step = 0, A.mCurrentCommand = 0, void A.closePrint()) : (console.log("重新检测", i["a"].printingStation, a["a"].state.project.currentPrint.sendCmdState, A.step), void setTimeout(function () {
      A.checkStopPrintNum--, A.mCurrentCommand = A.CMD_INQUIRY_STA, g["a"].SendCmd(A.mCurrentCommand, 0);
    }, 500));
  },
  stopPrintManual() {
    const A = this;
    A.waitStopPrintNum > 50 ? a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "检测设备超时!"
    }) : a["a"].state.project.currentPrint.sendCmdState ? setTimeout(() => {
      A.waitStopPrintNum++, A.waitStopPrintManual(), console.log("设备忙碌中，等待设备空闲在发送命令");
    }, 500) : i["a"].printingStation ? (A.mCurrentCommand = A.CMD_STOP_PRINT, g["a"].SendCmd(A.mCurrentCommand, 0), console.log("发送终止命令")) : this.closePrint();
  },
  waitStopPrintManual() {
    const A = this;
    A.devCheckErrMsg(!1) || (i["a"].printingStation ? a["a"].state.project.currentPrint.sendCmdState ? (A.waitStopPrintManual(), console.log("设备忙碌中，等待设备空闲在发送命令")) : setTimeout(() => {
      g["a"].SendCmd(A.CMD_INQUIRY_STA, 0);
    }, 500) : this.closePrint());
  },
  handleNotifyStop(A) {
    g["a"].handleInquiryStatus(A) && this.waitStopPrintManual();
  },
  getMatSn() {
    this.isStop = !1, a["a"].state.project.currentPrint.isStop = !1, this.mCurrentCommand = this.CMD_RETURN_MAT, this.sendCmd(this.mCurrentCommand, 0);
  },
  getDeviceInfo() {
    this.isStop = !1, a["a"].state.project.currentPrint.isStop = !1, this.mCurrentCommand = this.CMD_RETURN_DEVICE, this.sendCmd(this.CMD_RETURN_MAT, 0);
  },
  padRight(A, e, t) {
    let g = A + "";
    return g + new Array(e - g.length + 1).join(t, "");
  },
  byteToString(A, e, t) {
    let g = [];
    for (let i = e; i < e + t; i++) {
      if (0 == A[i]) break;
      g.push(String.fromCharCode(A[i]));
    }
    return g.join("");
  },
  bytesToString(A, e, t) {
    let g = [];
    for (let i = e; i < e + t; i++) {
      if (0 == A[i]) break;
      g.push(this.decimalToHex1(A[i]).padStart(2, "0"));
    }
    return g.join("");
  },
  decimalToHex1(A) {
    return A.toString(16);
  },
  getNextState() {
    return a["a"].state.project.currentPrint.num != i["a"].pPageCnt && !i["a"].BufFull;
  }
}

// --- CMD_INQUIRY_STA [ObjectProperty] (line 3, col 1846588) ---
CMD_INQUIRY_STA: 17


// =====================================================================
// SECTION: SendCmd - Send single command to printer
// (4 matches)
// =====================================================================

// --- SendCmd [ObjectMethod] (line 3, col 1846930) ---
SendCmd(A, e) {
  try {
    this.readType = !1;
    let t = new Uint8Array(8);
    t[0] = 192, t[1] = 64, t[2] = e >> 8, t[3] = e, t[4] = A, t[5] = 0, t[6] = 8, t[7] = 0, this.BulkWrite(t, t.length);
  } catch (t) {
    console.log("SendCmd", t), a["a"].commit("project/setHidDevice", {
      hidDevice: null
    }), a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "不可打印"
    });
  }
}

// --- sendCmd [ObjectMethod] (line 3, col 537103) ---
sendCmd(A, e) {
  if (!this.isStop) return a["a"].state.project.currentPrint.isStop ? (this.isStop = !0, this.step = 14, void this.handleStep()) : void (a["a"].state.project.currentPrint.sendCmdState || g["a"].SendCmd(A, e));
}

// --- sendCmd [ObjectMethod] (line 3, col 2844793) ---
sendCmd(A, e) {
  if (!this.isStop) return a["a"].state.project.currentPrint.isStop ? (this.isStop = !0, void this.stopPrintManual()) : void (a["a"].state.project.currentPrint.sendCmdState || g["a"].SendCmd(A, e));
}

// --- sendCmd [ObjectMethod] (line 3, col 3299170) ---
sendCmd(A, e) {
  if (!this.isStop) return a["a"].state.project.currentPrint.isStop ? (this.isStop = !0, this.step = 24, void this.handleStep()) : void (a["a"].state.project.currentPrint.sendCmdState || g["a"].SendCmd(A, e));
}


// =====================================================================
// SECTION: SendCmdTwo - Send command with two data parameters
// (1 match)
// =====================================================================

// --- SendCmdTwo [ObjectMethod] (line 3, col 1847242) ---
SendCmdTwo(A, e, t) {
  try {
    this.readType = !1;
    let g = new Uint8Array(10);
    g[0] = 192, g[1] = 64, g[2] = e >> 8, g[3] = e, g[4] = A, g[5] = 0, g[6] = 8, g[7] = 0, g[8] = t >> 8, g[9] = t, setTimeout(() => {
      this.BulkWrite(g, g.length);
    }, 50);
  } catch (g) {
    console.log("SendCmdTwo", g), this.hidDevice = null, a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "不可打印"
    });
  }
}


// =====================================================================
// SECTION: BulkWrite - Write bulk data to HID device
// (1 match)
// =====================================================================

// --- BulkWrite [ObjectMethod] (line 3, col 1847566) ---
async BulkWrite(A, e) {
  if (!a["a"].state.project.currentPrint.sendCmdState) try {
    let i = Math.floor(e / 65) + 1;
    for (var t = 0; t < i; t++) {
      let e = new Uint8Array(64);
      for (let g = 0; g < e.length; g++) t * e.length + g < A.length ? e[g] = A[t * e.length + g] : e[g] = 0;
      if (this.hidDevice && this.hidDevice.opened) {
        this.sendCmdState = !0;
        try {
          a["a"].commit("project/editCurrentSendCmdState", {
            sendCmdState: !0
          });
        } catch (g) {
          console.log("BulkWrite-editCurrentSendCmdState", g);
        }
        try {
          await this.hidDevice.sendReport(0, e);
        } catch (g) {
          a["a"].commit("project/editCurrentPrintState", {
            state: !1,
            errorMsg: "状态查询命令失败!"
          }), console.log("BulkWrite-sendReport", g, A);
        }
      } else a["a"].commit("project/editCurrentSendCmdState", {
        sendCmdState: !1
      }), console.log("无打印机", a["a"].state.project.currentPrint.sendCmdState), this.sendCmdState = !1;
    }
  } catch (g) {
    console.log("BulkWrite", g), this.hidDevice = null, a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "不可打印"
    });
  }
}


// =====================================================================
// SECTION: BulkWriteType - Write typed bulk data to HID device
// (1 match)
// =====================================================================

// --- BulkWriteType [ObjectMethod] (line 3, col 1848447) ---
async BulkWriteType(A, e, t = !0) {
  if (a["a"].state.project.currentPrint.sendCmdState) return;
  let g = [];
  try {
    this.type = 1;
    let I = Math.ceil(e / 65) + 1;
    for (var i = 0; i < I; i++) {
      if (e <= 64 * g.length) {
        console.log(e, 64 * g.length, "messages");
        break;
      }
      let t = new Uint8Array(64);
      for (let e = 0; e < t.length; e++) i * t.length + e < A.length ? t[e] = A[i * t.length + e] : t[e] = 0;
      g.push(t);
    }
    if (a["a"].state.project.currentPrint.sendCmdMatrix.total = g.length, g.length > 0) for (let A = 0; A < g.length; A++) {
      let e = g[A];
      this.hidDevice && this.hidDevice.opened ? (this.sendCmdState = t, a["a"].commit("project/editCurrentSendCmdState", {
        sendCmdState: t
      }), await this.hidDevice.sendReport(0, e), a["a"].state.project.currentPrint.sendCmdMatrix.num++) : (a["a"].commit("project/editCurrentSendCmdState", {
        sendCmdState: !1
      }), console.log("无打印机", a["a"].state.project.currentPrint.sendCmdState), this.sendCmdState = !1);
    }
  } catch (I) {
    console.log("BulkWriteType", I), this.hidDevice = null, a["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "不可打印"
    });
  }
}


// =====================================================================
// SECTION: doSupVanPrint - Main print orchestration
// (1 match)
// =====================================================================

// --- doSupVanPrint [ObjectMethod] (line 3, col 3298227) ---
doSupVanPrint(A, e, t, g, i, I, B, E, C) {
  const n = this;
  this.isStop = !1, n.statusNum = 100, this.waitImageNum = 0, this.imageDataList = a["a"].state.project.currentPrint.imageDataListAll.shift(), this.step = 1, this.RfidData = [], this.printerSn = t, this.paperType = g, this.gap = i, this.speed = I, this.isNetWork = B, this.isPrintFlow = E, this.objectLength = C, n.sendCmdNum = 0, n.cmdBufFullNum = 0, n.waitComOkNum = 20, n.checkStopPrintNum = 20, n.sedMatrixNum = 0, n.divRfidData = [], this.sendDataNum = 200, n.RfidData = [48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 121, 1, 1, 91, 235, 93, 155, 179, 48, 117, 1, 50, 50, 1, 3, 0, 224, 1, 0, 0, 164, 6, 176, 4, 23, 8, 17, 11, 48, 57, 0, 0, 135, 220, 151, 205, 1, 224, 159, 64, 149, 68, 77, 133, 236, 167, 205, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], B && (n.RfidData = [], n.RfidData = e), n.RfidData = e, this.imageDataList.length > 1 ? this.speed = 20 : this.speed = 60, this.startPrint();
}


// =====================================================================
// SECTION: createPrintData - Generate print data from DOM/canvas
// (1 match)
// =====================================================================

// --- createPrintData [ObjectMethod] (line 3, col 2342588) ---
async createPrintData(A, e, t, g, i, a, I, B) {
  const E = this;
  try {
    if (E.currentPrint.nextPrintState = !1, E.currentPrint.keepOnPrintPosition > i) return E.currentPrint.nextPrintState = !0, E.currentPrintNumAdd(), void document.getElementById(t).remove();
    E.currentPrint.printDPI <= 0 && (E.currentPrint.printDPI = this.projectModel.PrinterModel.DPI);
    const h = {
      quality: 1
    };
    let p;
    try {
      p = await this.htmlToImageSync(A, h);
    } catch (c) {
      console.error(c);
    }
    if (!p) return E.currentPrint.nextPrintState = !0, E.currentPrintNumAdd(), void document.getElementById(t).remove();
    var C = new Image();
    C.src = p, C = await E.loadImage(p);
    let M = A.style.width.replace("px", ""),
      D = A.style.height.replace("px", "");
    var n = document.createElement("canvas"),
      o = n.getContext("2d", {
        willReadFrequently: !0
      });
    let u = [],
      w = E.projectModel.PrinterModel.ProductId,
      R = f["a"].getDevType(w);
    if (f["a"].isSP(R)) {
      let A = a,
        e = I,
        t = E.projectModel.MatModel.Padding.Left,
        g = E.projectModel.MatModel.Padding.Right,
        i = E.projectModel.MatModel.Padding.Top,
        B = E.projectModel.MatModel.Padding.Bottom,
        l = 0,
        c = 672;
      if (0 == E.projectModel.MatModel.PaperDirection) {
        l = (Number.parseInt(E.paperType), Kg["a"].Plate, Number.parseInt((A - t - g + 2) * E.currentPrint.printDPI + .5)), n.width = l, n.height = c;
        let e = n.height / 2,
          i = C.height / 2;
        o.drawImage(C, -this.selectModule.Padding.Left * E.currentPrint.printDPI, e - i, M, D);
      } else {
        var Q = document.createElement("canvas"),
          s = Q.getContext("2d", {
            willReadFrequently: !0
          });
        Q.width = D, Q.height = M, s.save(), s.translate(Q.width / 2, Q.height / 2);
        let A = 270;
        const t = A * (Math.PI / 180);
        s.rotate(t), s.drawImage(C, -C.width / 2, -C.height / 2, C.width, C.height), s.restore();
        var r = this.convertCanvasToImage(Q);
        r = await E.loadImage(r.src), l = Number.parseInt((e - i - B + 2) * E.currentPrint.printDPI + .5), n.width = l, n.height = c;
        let a = Number.parseInt(i * E.currentPrint.printDPI),
          I = Number.parseInt(g * E.currentPrint.printDPI);
        o.drawImage(r, a, I, n.width, n.height, 0, 0, n.width, n.height);
      }
      u = o.getImageData(0, 0, n.width, n.height);
      for (let a = 0; a < u.data.length; a += 4) 0 === u.data[a + 3] && (u.data[a] = 255, u.data[a + 1] = 255, u.data[a + 2] = 255, u.data[a + 3] = 255);
      for (let a = 0; a < u.height; a++) for (let A = 0; A < u.width; A++) {
        let e = 4 * a * u.width + 4 * A,
          t = (u.data[e] + u.data[e + 1] + u.data[e + 2]) / 3;
        u.data[e] = t, u.data[e + 1] = t, u.data[e + 2] = t;
      }
    } else if (f["a"].isT5080(R)) {
      n.width = M, n.height = D, o.drawImage(C, 0, 0, M, D), u = o.getImageData(0, 0, M, D);
      for (let A = 0; A < u.height; A++) for (let e = 0; e < u.width; e++) {
        let t = 4 * A * u.width + 4 * e,
          g = (u.data[t] + u.data[t + 1] + u.data[t + 2]) / 3;
        u.data[t] = g, u.data[t + 1] = g, u.data[t + 2] = g;
      }
    } else if (f["a"].isTP(R)) {
      let A = 0,
        e = 144,
        t = a,
        g = E.projectModel.MatModel.Padding.Left,
        i = E.projectModel.MatModel.Padding.Right,
        I = Number.parseInt(t * E.currentPrint.printDPI + .5);
      A = Number.parseInt((t - g - i) * E.currentPrint.printDPI + .5), n.width = I, n.height = e;
      let B = C.width;
      B > n.width && (B /= 2);
      let Q = C.height;
      Q > n.height && (Q /= 2), o.drawImage(C, (n.width - B) / 2, (n.height - Q) / 2, M, D), u = o.getImageData(0, 0, n.width, n.height);
      for (let a = 0; a < u.data.length; a += 4) 0 === u.data[a + 3] && (u.data[a] = 255, u.data[a + 1] = 255, u.data[a + 2] = 255, u.data[a + 3] = 255);
      for (let a = 0; a < u.height; a++) for (let A = 0; A < u.width; A++) {
        let e = 4 * a * u.width + 4 * A,
          t = (u.data[e] + u.data[e + 1] + u.data[e + 2]) / 3;
        u.data[e] = t, u.data[e + 1] = t, u.data[e + 2] = t;
      }
    } else if (f["a"].isG(R)) {
      let A,
        e = a * E.currentPrint.printDPI,
        t = I * E.currentPrint.printDPI,
        g = E.projectModel.MatModel.Padding.Left * E.currentPrint.printDPI,
        i = E.projectModel.MatModel.Padding.Right * E.currentPrint.printDPI,
        Q = E.projectModel.MatModel.Padding.Top * E.currentPrint.printDPI,
        s = E.projectModel.MatModel.Padding.Bottom * E.currentPrint.printDPI;
      B ? (A = await Fi(C, g, Q, e, t - Q - s), M = e, D = t - Q - s) : (A = await Fi(C, g, Q, e - g - i, t - Q - s), M = e - g - i, D = t - Q - s);
      var l = new Image();
      if (l.src = A, l = await E.loadImage(A), this.autoIdentifyMat.LableSn && this.gbiaoshipai.includes(this.autoIdentifyMat.LableSn)) {
        let A = 42,
          e = 2,
          t = 2;
        M = (A - e - t) * E.currentPrint.printDPI;
      }
      a = M / E.currentPrint.printDPI, I = Math.ceil(D / E.currentPrint.printDPI), n.width = M, n.height = D, o.drawImage(l, 0, 0, M, D), u = o.getImageData(0, 0, M, D);
      for (let a = 0; a < u.height; a++) for (let A = 0; A < u.width; A++) {
        let e = 4 * a * u.width + 4 * A,
          t = (u.data[e] + u.data[e + 1] + u.data[e + 2]) / 3;
        u.data[e] = t, u.data[e + 1] = t, u.data[e + 2] = t;
      }
    }
    if (o.putImageData(u, 0, 0), !u) return;
    let G = Object(m["a"])(d["b"]);
    if (E.paperRotate ? (G.Width = a, G.Height = I) : (G.Width = I, G.Height = a), G.Gap = E.paperGap, G.DPI = E.currentPrint.printDPI, G.Rotate = 1, G.ImagePixel = u.data, G.OverturnType = 0, G.Density = E.concentration, G.PaperType = Number.parseInt(E.paperType), G.Copies = 1, G.HorizontalOffset = E.horizontal_offset, G.VerticalOffset = E.vertical_offset, G.HasBarCode = g, G.ImgWidth = u.width, G.ImgHeight = u.height, G.CutType = E.cutType, E.anyPrint) {
      if (!E.formPause.state) return;
      E.toPrinter(G, E.printLength, i), document.getElementById(t).remove();
    } else {
      for (let A = 0; A < e; A++) E.toPrinter(G, E.printLength, i);
      document.getElementById(t).remove();
    }
    await this.waitSinglePrintEnd();
  } catch (c) {
    console.error(c);
  }
}


// =====================================================================
// SECTION: printData - Image pixel encoding for printer
// (1 match)
// =====================================================================

// --- printData [ObjectMethod] (line 3, col 958719) ---
printData(A) {
  const e = this;
  let t = A.length,
    g = A[0].length,
    i = 1,
    a = t / 8;
  e.bt = new Array(a * g).fill(0);
  for (let I = 0; I < g; I++) for (let g = 0; g < t; g++) {
    let t = A[g][I];
    1 == t && (1 == i && (e.marginTop = I, i = 0), e.marginBottom = I);
    let B = a * I + Math.floor(g / 8),
      E = g % 8,
      C = e.bt[B];
    C |= t << E, e.bt[B] = C;
  }
  return e.marginBottom = e.marginBottom + 1, e.marginBottom > g && (e.marginBottom = g), e.marginBottom = g - e.marginBottom, e.bt;
}


// =====================================================================
// SECTION: connectDevice - Device connection flow
// (1 match)
// =====================================================================

// --- connectDevice [ObjectMethod] (line 3, col 2384968) ---
async connectDevice() {
  this.editIsConnDevice({
    isConnDevice: !0
  }), await f["a"].connectHIDDevice(), await f["a"].hidDeviceOpen(), await this.waitForConnectDevice();
}


// =====================================================================
// SECTION: connectHIDDevice - WebHID device connection
// (4 matches)
// =====================================================================

// --- connectHIDDevice [ObjectMethod] (line 3, col 1851258) ---
async connectHIDDevice() {
  return new Promise(async (A, e) => {
    try {
      this.hidDevice = null, a["a"].commit("project/setHidDevice", {
        hidDevice: null
      });
      let A = await navigator.hid.getDevices(),
        e = !1;
      if (A) for (let g = A.length - 1; g >= 0; g--) {
        const I = A[g];
        if (I) {
          let B = I.productId,
            E = I.vendorId;
          if (a["a"].state.project.packageName == s["a"].PackageName_KataSymbolEditor && 8306 != B && 8307 != B && 8309 != B && 8310 != B && 8314 != B) continue;
          if (i["a"] && i["a"].length > 0) {
            for (let I = 0; I < i["a"].length; I++) {
              const C = i["a"][I];
              if (C && C.ProductId == B && C.VendorId == E) {
                this.hidDevice = A[g];
                try {
                  let A = await this.checkHid();
                  if (!A) {
                    this.hidDevice = null, console.log("打开设备失败，继续寻找下一个");
                    continue;
                  }
                } catch (t) {
                  this.hidDevice = null, console.log("打开设备失败，继续寻找下一个");
                  continue;
                }
                console.log("Connected to HID Device:", this.hidDevice), e = !0, this.sendCmdState = !1, a["a"].commit("project/editCurrentSendCmdState", {
                  sendCmdState: !1
                });
                break;
              }
            }
            if (e) break;
          }
        }
      }
    } catch (t) {
      console.error("Error connecting to HID device:", t);
    }
    this.hidDevice && (this.hidDevice.opened && this.hidDevice.close(), this.returnCmd()), A(this.hidDevice);
  });
}

// --- connectHIDDevice [ObjectMethod] (line 3, col 2356729) ---
async connectHIDDevice() {
  if (!this.currentPrint.isConnDevice) return;
  await f["a"].connectHIDDevice();
  let A = !1;
  1 == this.projectModel.MatModel.Diy && (A = !0), k.sendRfid(this.projectModel.MatModel, A);
}

// --- connectHIDDevice [ObjectMethod] (line 3, col 2535683) ---
async connectHIDDevice() {
  g["a"].state.project.currentPrint.isConnDevice ? await B["a"].connectHIDDevice().then(A => {
    if (this.hidDevice = A, this.hidDevice) {
      let A = E["a"].filter(A => A.ProductId == this.hidDevice.productId && A.VendorId == this.hidDevice.vendorId);
      A && A.length > 0 && (this.initPrinter(A), g["a"].commit("project/setHidDevice", {
        hidDevice: this.hidDevice
      }), g["a"].commit("project/editPrinterModelConn", {
        connectionStatus: !0
      }));
    } else g["a"].commit("project/editPrinterModelConn", {
      connectionStatus: !1
    }), console.log("没有机器");
  }) : g["a"].commit("project/editPrinterModelConn", {
    connectionStatus: !1
  });
}

// --- connectHIDDeviceForId [ObjectMethod] (line 3, col 1852285) ---
async connectHIDDeviceForId(A, e) {
  return new Promise(async (t, g) => {
    try {
      let t = await navigator.hid.getDevices(),
        g = !1;
      if (t) for (let i = 0; i < t.length; i++) {
        const I = t[i];
        if (I && I.productId == e && I.vendorId == A) {
          this.hidDevice = t[i], g = !0, this.sendCmdState = !1, a["a"].commit("project/editCurrentSendCmdState", {
            sendCmdState: !1
          });
          break;
        }
      }
    } catch (i) {
      console.error("Error connecting to HID device:", i);
    }
    this.hidDevice && (t(this.hidDevice), this.hidDevice.opened || this.hidDevice.open(), this.returnCmd());
  });
}


// =====================================================================
// SECTION: autoConnectDevice - Automatic device reconnection
// (1 match)
// =====================================================================

// --- autoConnectDevice [ObjectMethod] (line 3, col 2383182) ---
autoConnectDevice() {
  const A = this;
  return new Promise(e => {
    setTimeout(async () => {
      A.running || (A.setDefaultDevice(), A.running || (A.setLastPrintDevice(), A.running || (A.setDefaultMat(), A.running || (A.setLastPrintMat(), A.running || (await A.connectDevice(), A.running || (await A.identifyMat(), A.running || (A.connectVerify(), A.running || (A.setPage(), A.running = !0, Object(m["g"])(), e("自动连接任务完成")))))))));
    }, 0);
  });
}


// =====================================================================
// SECTION: getRfidData - RFID data preparation
// (1 match)
// =====================================================================

// --- getRfidData [ObjectMethod] (line 3, col 1973486) ---
getRfidData() {
  if (h["a"].state.project.projectModel.PrinterModel) {
    let A = h["a"].state.project.projectModel.PrinterModel.ProductId,
      e = f["a"].getDevType(A);
    if (f["a"].isT5080(e)) {
      let A = this.originalHeight,
        e = this.originalWidth,
        t = h["a"].state.project.projectModel.MatModel.PaperDirection,
        g = this.mPaperType,
        i = this.mGap,
        a = h["a"].state.project.projectModel.MatModel.ID;
      return this.getT50PlusRFIDData(A, e, t, g, i, 0, a);
    }
  }
  return [];
}


// =====================================================================
// SECTION: getLzma - LZMA compression wrapper
// (1 match)
// =====================================================================

// --- getLzma [ObjectMethod] (line 3, col 2531805) ---
getLzma(A) {
  return i.a.compress(A, 9);
}


// =====================================================================
// SECTION: HID Device Open/Close/Init
// (6 matches)
// =====================================================================

// --- hidDeviceOpen [ObjectMethod] (line 3, col 1852905) ---
async hidDeviceOpen() {
  return new Promise(async (A, e) => {
    try {
      this.hidDevice && !this.hidDevice.opened && (await this.hidDevice.open(), a["a"].commit("project/editCurrentSendCmdState", {
        sendCmdState: !1
      }), this.sendCmdState = !1);
    } catch (t) {
      a["a"].state.project.projectModel.PrinterModel && a["a"].commit("project/editPrinterModelConn", {
        connectionStatus: !1
      }), console.error("Error hidDeviceOpen:", t);
    }
    A(this.hidDevice);
  });
}

// --- hidDeviceOpen [ObjectMethod] (line 3, col 2536265) ---
async hidDeviceOpen() {
  if (!g["a"].state.project.currentPrint.isConnDevice) return void g["a"].commit("project/editPrinterModelConn", {
    connectionStatus: !1
  });
  console.log("准备打开设备", this.hidDevice);
  const A = this;
  if (null == A.hidDevice) {
    if (A.connectMaxNum > 10) return;
    A.connectMaxNum > 2 && A.connectHIDDevice(), await setTimeout(() => {
      A.connectMaxNum++, A.hidDeviceOpen();
    }, 500);
  } else await B["a"].hidDeviceOpen().then(async e => {
    if (A.hidDevice = e, console.log("打开设备", A.hidDevice), A.connectMaxNum > 10) return g["a"].commit("project/editCurrentPrintState", {
      state: !1,
      errorMsg: "未连接"
    }), void (this.connectMaxNum = 0);
    A.hidDevice && !A.hidDevice.opened && (await setTimeout(() => {
      A.connectMaxNum++, A.hidDeviceOpen();
    }, 500));
  });
}

// --- hidDeviceClose [ObjectMethod] (line 3, col 1853307) ---
async hidDeviceClose() {
  return new Promise(async (A, e) => {
    try {
      this.hidDevice && this.hidDevice.opened && (await this.hidDevice.close(), a["a"].commit("project/editCurrentSendCmdState", {
        sendCmdState: !1
      }), this.sendCmdState = !1);
    } catch (t) {
      console.error("Error hidDeviceClose:", t);
    }
    A(this.hidDevice);
  });
}

// --- initPrinter [ObjectMethod] (line 3, col 2536948) ---
initPrinter(A) {
  let e = Object(i["a"])(a["f"]),
    t = this.getDevType(this.hidDevice.productId);
  t && t > 0 && (e.PrinterSerie = this.getPrinterSerie(t), e.PrinterSerieName = this.getPrinterSerieName(e.PrinterSerie), e.PrinterType = e.PrinterSerie, e.PrinterName = this.getPrinterName(t, this.hidDevice.productId), e.PrinterImageUrl = A[0].Image, e.SerialNumber = "", e.DPI = this.getPrinterDPI(e.PrinterSerie), e.ProductId = this.hidDevice.productId, e.VendorId = this.hidDevice.vendorId, e.ConnectionStatus = !1, g["a"].commit("project/setPrinterModel", {
    printerModel: e
  }));
}

// --- returnCmd [ObjectMethod] (line 3, col 1849406) ---
returnCmd() {
  const A = this;
  let e = this.hidDevice.productId,
    t = I["a"].getDevType(e);
  function g(e) {
    let t = new Uint8Array(e.data.buffer);
    A.readData = [];
    for (var g = 0; g < 8; g++) t.length > g ? A.readData[g] = t[g] : A.readData[g] = 0;
    A.readType = !0, a["a"].commit("project/editCurrentSendCmdState", {
      sendCmdState: !1
    }), a["a"].state.project.currentPrint.isStop ? B["a"].handleNotifyStop(t) : B["a"].handleNotify(t);
  }
  function i(e) {
    let t = new Uint8Array(e.data.buffer);
    A.readData = [];
    for (var g = 0; g < 8; g++) t.length > g ? A.readData[g] = t[g] : A.readData[g] = 0;
    A.readType = !0, a["a"].commit("project/editCurrentSendCmdState", {
      sendCmdState: !1
    }), A.sendCmdState = !1, n["a"].handleNotify(t);
  }
  function s(e) {
    console.log("inputreportTP86A");
    let t = new Uint8Array(e.data.buffer);
    A.readData = [];
    for (var g = 0; g < 8; g++) t.length > g ? A.readData[g] = t[g] : A.readData[g] = 0;
    A.readType = !0, a["a"].commit("project/editCurrentSendCmdState", {
      sendCmdState: !1
    }), a["a"].state.project.currentPrint.isStop ? E["a"].handleNotifyStop(t) : E["a"].handleNotify(t);
  }
  function r(e) {
    console.log("inputreportTP");
    let t = new Uint8Array(e.data.buffer);
    A.readData = [];
    for (var g = 0; g < 8; g++) t.length > g ? A.readData[g] = t[g] : A.readData[g] = 0;
    A.readType = !0, a["a"].commit("project/editCurrentSendCmdState", {
      sendCmdState: !1
    }), a["a"].state.project.currentPrint.isStop ? C["a"].handleNotifyStop(t) : C["a"].handleNotify(t);
  }
  function l(e) {
    let t = new Uint8Array(e.data.buffer);
    A.readData = [];
    for (var g = 0; g < 8; g++) t.length > g ? A.readData[g] = t[g] : A.readData[g] = 0;
    A.readType = !0, a["a"].commit("project/editCurrentSendCmdState", {
      sendCmdState: !1
    }), A.sendCmdState = !1, o["a"].handleNotify(t);
  }
  I["a"].isT5080(t) ? A.hidDevice.oninputreport = i : I["a"].isSP(t) ? A.hidDevice.oninputreport = g : I["a"].isTP(t) ? t == Q["a"].Supvan_TP86A || t == Q["a"].Supvan_TP80A || t == Q["a"].Supvan_TP56 || t == Q["a"].Supvan_TP76i_G ? A.hidDevice.oninputreport = s : A.hidDevice.oninputreport = r : I["a"].isG(t) && (A.hidDevice.oninputreport = l);
}

// --- getPrinterState [ObjectMethod] (line 3, col 2544734) ---
async getPrinterState(A, e) {
  return await B["a"].getPrinter().then(async A => {
    this.hidDevice = A;
  }), !!this.hidDevice && this.hidDevice.opened;
}


// =====================================================================
// SECTION: Status/Response Handling
// (11 matches)
// =====================================================================

// --- handleStep [ObjectMethod] (line 3, col 537308) ---
handleStep() {
  const A = this;
  switch (A.step) {
    case 0:
      break;
    case 1:
      A.mCurrentCommand = A.CMD_INQUIRY_STA, A.sendCmd(A.mCurrentCommand, 0);
      break;
    case 2:
      A.mCurrentCommand = A.CMD_CHECK_DEVICE, A.sendCmd(A.mCurrentCommand, 0);
      break;
    case 3:
      A.waitComOk();
      break;
    case 4:
      if (A.devCheckErrMsg()) return void this.closePrint();
      A.startG();
      break;
    case 5:
      A.WaitDevRun();
      break;
    case 6:
      if (A.devCheckErrMsg()) return void this.closePrint();
      A.handleTransferStep();
      break;
    case 7:
      if (A.devCheckErrMsg()) return void this.closePrint();
      this.sendMatrix();
      break;
    case 8:
      if (A.devCheckErrMsg()) return void this.closePrint();
      this.cmdbuffull();
      break;
    case 9:
      if (A.devCheckErrMsg()) return void this.closePrint();
      A.handleTransferNext();
      break;
    case 10:
      if (A.devCheckErrMsg()) return void this.closePrint();
      A.waitNewPrint();
      break;
    case 11:
      A.printEnd();
      break;
    case 12:
      A.waitClose();
      break;
    case 13:
      A.closePrint();
      break;
    case 14:
      this.checkStopPrint();
      break;
    case 15:
      this.stopPrintManual();
      break;
  }
}

// --- handleStep [ObjectMethod] (line 3, col 2848430) ---
handleStep() {
  const A = this;
  switch (A.step) {
    case 0:
      break;
    case 1:
      A.mCurrentCommand = A.CMD_CHECK_DEVICE, A.sendCmd(A.mCurrentCommand, 0);
      break;
    case 2:
      A.waitComOk();
      break;
    case 3:
      if (A.devCheckErrMsg()) return void this.closePrint();
      A.needClr();
      break;
    case 4:
      break;
    case 5:
      A.sendPrintMaterial();
      break;
    case 6:
      A.startSP();
      break;
    case 7:
      A.checkPrtSta();
      break;
    case 8:
      A.spPrintImage();
      break;
    case 9:
      A.handleTransferStep();
      break;
    case 10:
      A.sendMatrix();
      break;
    case 11:
      A.cmdbuffull();
      break;
    case 12:
      A.waitNewPrint();
      break;
    case 13:
      A.printEnd();
      break;
    case 14:
      A.handleTransferNext();
      break;
  }
}

// --- handleStep [ObjectMethod] (line 3, col 3008051) ---
handleStep() {
  const A = this;
  switch (A.step) {
    case 0:
      break;
    case 1:
      A.ma = Number.parseInt(I["a"].getTPMaterialTypeCode() << 8) + Number.parseInt(this.pageObject.CutType), A.mCurrentCommand = A.CMD_CHECK_DEVICE, A.sendCmd(A.mCurrentCommand, A.ma);
      break;
    case 2:
      A.waitComOk();
      break;
    case 3:
      if (A.devCheckErrMsg()) return void this.closePrint();
      this.step = 4, this.handleStep();
      break;
    case 4:
      A.startTP();
      break;
    case 5:
      A.checkPrtSta();
      break;
    case 6:
      A.tpPrintImage();
      break;
    case 7:
      A.handleTransferStep();
      break;
    case 8:
      A.sendMatrix();
      break;
    case 9:
      A.cmdbuffull();
      break;
    case 10:
      A.handleTransferNext();
      break;
    case 11:
      A.waitNewPrint();
      break;
    case 12:
      A.printEnd();
      break;
  }
}

// --- handleStep [ObjectMethod] (line 3, col 3097827) ---
handleStep() {
  const A = this;
  switch (A.step) {
    case 0:
      break;
    case 1:
      A.ma = Number.parseInt(I["a"].getTPMaterialTypeCode() << 8) + Number.parseInt(this.pageObject.CutType), A.mCurrentCommand = A.CMD_CHECK_DEVICE, A.sendCmd(A.mCurrentCommand, A.ma);
      break;
    case 2:
      A.waitComOk();
      break;
    case 3:
      if (A.devCheckErrMsg()) return void this.closePrint();
      this.step = 4, this.handleStep();
      break;
    case 4:
      A.startTP();
      break;
    case 5:
      A.checkPrtSta();
      break;
    case 6:
      A.handleTransferStep();
      break;
    case 7:
      A.sendMatrix();
      break;
    case 8:
      A.cmdbuffull();
      break;
    case 9:
      A.handleTransferNext();
      break;
    case 10:
      A.waitNewPrint();
      break;
    case 11:
      A.printEnd();
      break;
  }
}

// --- handleStep [ObjectMethod] (line 3, col 3299375) ---
handleStep() {
  const A = this;
  switch (A.step) {
    case 0:
      break;
    case 1:
      A.mCurrentCommand = A.CMD_CHECK_DEVICE, A.sendCmd(A.mCurrentCommand, 0);
      break;
    case 2:
      A.waitComOk();
      break;
    case 3:
      if (A.devCheckErrMsg()) return void this.closePrint();
      if (i["a"].printingStation) return void a["a"].commit("project/editCurrentPrintState", {
        state: !1,
        errorMsg: "打印中，稍后再试"
      });
      A.startT5080();
      break;
    case 4:
      if (A.devCheckErrMsg()) return void this.closePrint();
      A.handleTransferStep();
      break;
    case 5:
      if (A.devCheckErrMsg()) return void this.closePrint();
      this.sendMatrix();
      break;
    case 6:
      if (A.devCheckErrMsg()) return void this.closePrint();
      this.cmdbuffull();
      break;
    case 7:
      if (A.devCheckErrMsg()) return void this.closePrint();
      A.handleTransferNext();
      break;
    case 8:
      if (A.devCheckErrMsg()) return void this.closePrint();
      A.waitNewPrint();
      break;
    case 9:
      A.printEnd();
      break;
    case 10:
      A.waitClose();
      break;
    case 11:
      A.closePrint();
      break;
    case 20:
      A.setRfidData();
      break;
    case 21:
      A.sendRfidData();
      break;
    case 22:
      A.sendDivRfidData();
      break;
    case 23:
      a["a"].state.mat.sendRfidState = !0, this.closePrint();
      break;
    case 24:
      this.checkStopPrint();
      break;
    case 25:
      this.stopPrintManual();
      break;
  }
}

// --- waitClose [ObjectMethod] (line 3, col 542577) ---
waitClose() {
  console.log(i.PrtSta, this.waitEndPrintNum), i.PrtSta && this.waitEndPrintNum < 20 ? setTimeout(() => {
    this.step = 12, this.mCurrentCommand = this.CMD_INQUIRY_STA, g["a"].SendCmd(this.mCurrentCommand, 0), this.waitEndPrintNum++;
  }, 200) : (this.mCurrentCommand = 0, this.closePrint());
}

// --- waitClose [ObjectMethod] (line 3, col 3303926) ---
waitClose() {
  console.log(i["a"].printingStation, this.waitEndPrintNum), i["a"].printingStation && this.waitEndPrintNum < 20 ? setTimeout(() => {
    this.step = 10, this.mCurrentCommand = this.CMD_INQUIRY_STA, g["a"].SendCmd(this.mCurrentCommand, 0), this.waitEndPrintNum++;
  }, 200) : (this.mCurrentCommand = 0, this.closePrint());
}

// --- closePrint [ObjectMethod] (line 3, col 547096) ---
closePrint() {
  const A = this;
  i.PrtSta ? setTimeout(function () {
    this.step = 13, console.log("等待设备释放", i.PrtSta), A.mCurrentCommand = A.CMD_INQUIRY_STA, g["a"].SendCmd(A.mCurrentCommand, 0);
  }, 500) : g["a"].hidDeviceClose().then(A => {
    this.hidDevice = A, console.log("设备已关闭", this.hidDevice, i.PrtSta), a["a"].state.project.currentPrint.isPrintEnd = !0, a["a"].state.project.currentPrint.close = !0;
  });
}

// --- closePrint [ObjectMethod] (line 3, col 2355873) ---
closePrint() {
  this.editKeepOnPrintPosition({
    keepOnPrintPosition: 0
  }), this.currentPrint.isPrintEnd && (this.dialogVisiblePause = !1), this.currentPrint.isStop && this.currentPrint.close && (this.dialogVisiblePause = !1), this.closePrintBtnDisabled = !0, null == this.currentPrint.hidDevice ? this.dialogVisiblePause = !1 : (this.editCurrentPrintIsStop({
    isStop: !0
  }), this.waitClosePrint());
}

// --- closePrint [ObjectMethod] (line 3, col 2854085) ---
closePrint() {
  g["a"].hidDeviceClose().then(A => {
    this.hidDevice = A, console.log("设备已关闭", this.hidDevice), a["a"].state.project.currentPrint.isPrintEnd = !0, a["a"].state.project.currentPrint.close = !0;
  });
}

// --- closePrint [ObjectMethod] (line 3, col 3308834) ---
closePrint() {
  const A = this;
  i["a"].printingStation ? setTimeout(function () {
    this.step = 11, console.log("等待设备释放", i["a"].printingStation), A.mCurrentCommand = A.CMD_INQUIRY_STA, g["a"].SendCmd(A.mCurrentCommand, 0);
  }, 500) : g["a"].hidDeviceClose().then(A => {
    this.hidDevice = A, console.log("设备已关闭", this.hidDevice, i["a"].printingStation), a["a"].state.project.currentPrint.isPrintEnd = !0, a["a"].state.project.currentPrint.close = !0;
  });
}


// =====================================================================
// SECTION: Connection Helpers
// (5 matches)
// =====================================================================

// --- waitForConnectDevice [ObjectMethod] (line 3, col 2386869) ---
async waitForConnectDevice() {
  return new Promise(async A => {
    this.connectDeviceNum = 0, await this.loopWaitForConnectDevice(A);
  });
}

// --- loopWaitForConnectDevice [ObjectMethod] (line 3, col 2386993) ---
async loopWaitForConnectDevice(A) {
  const e = this;
  if (e.connectDeviceNum > 5) return console.log("连接超时"), void A();
  e.printerModel && e.printerModel.ProductId && e.printerModel.VendorId && e.printerModel.ConnectionStatus ? (console.log("连接成功"), A()) : setTimeout(() => {
    e.connectDeviceNum++, e.loopWaitForConnectDevice(A);
  }, 50);
}

// --- sendMatrix [ObjectMethod] (line 3, col 545218) ---
sendMatrix() {
  this.sedMatrixNum > 50 ? console.log("设备超时，发送字模失败") : a["a"].state.project.currentPrint.sendCmdState ? (console.log("重试sedMatrix"), setTimeout(() => {
    this.sedMatrixNum++, this.sendMatrix();
  }, 200)) : setTimeout(() => {
    this.sedMatrixNum = 0, this.mCurrentCommand = this.CMD_TRANSFER_ONE, g["a"].BulkWriteType(this.imageData, this.imageData.length);
  }, 100);
}

// --- sendMatrix [ObjectMethod] (line 3, col 3013660) ---
sendMatrix() {
  this.sedMatrixNum > 50 ? console.log("设备超时，发送字模失败") : this.devCheckErrMsg() ? this.closePrint() : a["a"].state.project.currentPrint.sendCmdState ? (console.log("重试sedMatrix"), setTimeout(() => {
    this.sedMatrixNum++, this.sendMatrix();
  }, 200)) : setTimeout(() => {
    this.sedMatrixNum = 0, g["a"].BulkWriteType(this.imageData, this.imageData.length, !1), setTimeout(() => {
      this.step = 9, this.handleStep();
    }, 100);
  }, 10);
}

// --- sendMatrix [ObjectMethod] (line 3, col 3103102) ---
sendMatrix() {
  this.sedMatrixNum > 50 ? console.log("设备超时，发送字模失败") : this.devCheckErrMsg() ? this.closePrint() : a["a"].state.project.currentPrint.sendCmdState ? (console.log("重试sedMatrix"), setTimeout(() => {
    this.sedMatrixNum++, this.sendMatrix();
  }, 200)) : setTimeout(() => {
    this.sedMatrixNum = 0, g["a"].BulkWriteType(this.imageDataList, this.imageDataList.length, !1), setTimeout(() => {
      this.step = 8, this.handleStep();
    }, 50);
  }, 100);
}


// =====================================================================
// SECTION: USB VID/PID Constants
// USB Vendor ID / Product ID filter objects
// (19 matches)
// =====================================================================

// --- USB_VID_PID [Object with vendorId/productId numeric literals] (line 3, col 591160) ---
{
  PrinterName: "T50M Plus",
  Image: "KataSymbol.Editor" == i["a"].state.project.packageName ? t("dee1") : t("380f"),
  DPI: "",
  PrinterSn: "",
  ProductId: 8306,
  VendorId: 6176,
  PrinterSerie: g["c"].T50Plus,
  label: "labelMachine",
  id: 0,
  Default: !1,
  Show: !0
}

// --- USB_VID_PID [Object with vendorId/productId numeric literals] (line 3, col 591388) ---
{
  PrinterName: "T50s",
  Image: t("48cb"),
  DPI: "",
  PrinterSn: "",
  ProductId: 8308,
  VendorId: 6176,
  PrinterSerie: g["c"].T50Pro,
  label: "labelMachine",
  id: 1,
  Default: !1,
  Show: !1
}

// --- USB_VID_PID [Object with vendorId/productId numeric literals] (line 3, col 591546) ---
{
  PrinterName: "T50M Pro",
  Image: t("48cb"),
  DPI: "",
  PrinterSn: "",
  ProductId: 8307,
  VendorId: 6176,
  PrinterSerie: g["c"].T50Pro,
  label: "labelMachine",
  id: 2,
  Default: !1,
  Show: !1
}

// --- USB_VID_PID [Object with vendorId/productId numeric literals] (line 3, col 591708) ---
{
  PrinterName: "T50M Pro",
  Image: t("48cb"),
  DPI: "",
  PrinterSn: "",
  ProductId: 8310,
  VendorId: 6176,
  PrinterSerie: g["c"].T50Pro,
  label: "labelMachine",
  id: 3,
  Default: !0,
  Show: !0
}

// --- USB_VID_PID [Object with vendorId/productId numeric literals] (line 3, col 591870) ---
{
  PrinterName: "T50M Pro",
  Image: t("48cb"),
  DPI: "",
  PrinterSn: "",
  ProductId: 8311,
  VendorId: 6176,
  PrinterSerie: g["c"].T50Pro,
  label: "labelMachine",
  id: 4,
  Default: !1,
  Show: !1
}

// --- USB_VID_PID [Object with vendorId/productId numeric literals] (line 3, col 592032) ---
{
  PrinterName: "T80M",
  Image: t("b397"),
  DPI: "",
  PrinterSn: "",
  ProductId: 8309,
  VendorId: 6176,
  PrinterSerie: g["c"].T80Pro,
  label: "labelMachine",
  id: 5,
  Default: !1,
  Show: !0
}

// --- USB_VID_PID [Object with vendorId/productId numeric literals] (line 3, col 592190) ---
{
  PrinterName: "T80MPro",
  Image: t("c8a2"),
  DPI: "",
  PrinterSn: "",
  ProductId: 8314,
  VendorId: 6176,
  PrinterSerie: g["c"].T80Pro,
  label: "labelMachine",
  id: 16,
  Default: !1,
  Show: !0
}

// --- USB_VID_PID [Object with vendorId/productId numeric literals] (line 3, col 592352) ---
{
  PrinterName: "SP650",
  Image: t("cbaa"),
  DPI: "",
  PrinterSn: "",
  ProductId: 8254,
  VendorId: 6176,
  PrinterSerie: g["c"].SP,
  label: "signageMachine",
  id: 6,
  Default: !1,
  Show: !0
}

// --- USB_VID_PID [Object with vendorId/productId numeric literals] (line 3, col 592509) ---
{
  PrinterName: "TP86A",
  Image: t("823b"),
  DPI: "",
  PrinterSn: "",
  ProductId: 8239,
  VendorId: 6176,
  PrinterSerie: g["c"].TP,
  label: "tpMachine",
  id: 7,
  Default: !1,
  Show: !0
}

// --- USB_VID_PID [Object with vendorId/productId numeric literals] (line 3, col 592661) ---
{
  PrinterName: "TP86A",
  Image: t("823b"),
  DPI: "",
  PrinterSn: "",
  ProductId: 8321,
  VendorId: 6176,
  PrinterSerie: g["c"].TP,
  label: "tpMachine",
  id: 8,
  Default: !1,
  Show: !0
}

// --- USB_VID_PID [Object with vendorId/productId numeric literals] (line 3, col 592813) ---
{
  PrinterName: "TP80A",
  Image: t("aedd"),
  DPI: "",
  PrinterSn: "",
  ProductId: 8320,
  VendorId: 6176,
  PrinterSerie: g["c"].TP,
  label: "tpMachine",
  id: 9,
  Default: !1,
  Show: !0
}

// --- USB_VID_PID [Object with vendorId/productId numeric literals] (line 3, col 592965) ---
{
  PrinterName: "TP80A",
  Image: t("aedd"),
  DPI: "",
  PrinterSn: "",
  ProductId: 8238,
  VendorId: 6176,
  PrinterSerie: g["c"].TP,
  label: "tpMachine",
  id: 10,
  Default: !1,
  Show: !0
}

// --- USB_VID_PID [Object with vendorId/productId numeric literals] (line 3, col 593118) ---
{
  PrinterName: "TP76I",
  Image: t("734f"),
  DPI: "",
  PrinterSn: "",
  ProductId: 8236,
  VendorId: 6176,
  PrinterSerie: g["c"].TP,
  label: "tpMachine",
  id: 11,
  Default: !1,
  Show: !0
}

// --- USB_VID_PID [Object with vendorId/productId numeric literals] (line 3, col 593271) ---
{
  PrinterName: "TP76I",
  Image: t("734f"),
  DPI: "",
  PrinterSn: "",
  ProductId: 8327,
  VendorId: 6176,
  PrinterSerie: g["c"].TP,
  label: "tpMachine",
  id: 17,
  Default: !1,
  Show: !0
}

// --- USB_VID_PID [Object with vendorId/productId numeric literals] (line 3, col 593424) ---
{
  PrinterName: "G11 Pro",
  Image: t("66d2"),
  DPI: "",
  PrinterSn: "",
  ProductId: 8336,
  VendorId: 6176,
  PrinterSerie: g["c"].G_DPI_200,
  label: "gMachines",
  id: 12,
  Default: !1,
  Show: !0
}

// --- USB_VID_PID [Object with vendorId/productId numeric literals] (line 3, col 593586) ---
{
  PrinterName: "G15 Pro",
  Image: t("0abe"),
  DPI: "",
  PrinterSn: "",
  ProductId: 8338,
  VendorId: 6176,
  PrinterSerie: g["c"].G_DPI_200,
  label: "gMachines",
  id: 13,
  Default: !1,
  Show: !0
}

// --- USB_VID_PID [Object with vendorId/productId numeric literals] (line 3, col 593748) ---
{
  PrinterName: "G15 MPro",
  Image: t("e48e"),
  DPI: "",
  PrinterSn: "",
  ProductId: 8339,
  VendorId: 6176,
  PrinterSerie: g["c"].G_DPI_200,
  label: "gMachines",
  id: 14,
  Default: !1,
  Show: !0
}

// --- USB_VID_PID [Object with vendorId/productId numeric literals] (line 3, col 593911) ---
{
  PrinterName: "G18 Pro",
  Image: t("eccd"),
  DPI: "",
  PrinterSn: "",
  ProductId: 8337,
  VendorId: 6176,
  PrinterSerie: g["c"].G_DPI_200,
  label: "gMachines",
  id: 15,
  Default: !1,
  Show: !0
}

// --- USB_VID_PID [Object with vendorId/productId numeric literals] (line 3, col 3791289) ---
{
  PrinterSerie: -1,
  PrinterSerieName: "",
  PrinterType: -1,
  PrinterName: "",
  PrinterImageUrl: "",
  SerialNumber: "",
  DPI: 0,
  ProductId: 0,
  VendorId: 0,
  ConnectionStatus: !1
}


// =====================================================================
// SECTION: HID requestDevice / getDevices Calls
// WebHID API calls that enumerate or request devices
// (2 matches)
// =====================================================================

// --- getDevices_call [HID getDevices call] (line 3, col 1851394) ---
let A = await navigator.hid.getDevices(),
  e = !1;

// --- getDevices_call [HID getDevices call] (line 3, col 1852354) ---
let t = await navigator.hid.getDevices(),
  g = !1;



// =====================================================================
// EXTRACTION SUMMARY
// =====================================================================
// CMD_CONSTANTS_OBJECT: 5 unique match(es)
// sendCmd: 3 unique match(es)
// handleStep: 5 unique match(es)
// waitClose: 2 unique match(es)
// sendMatrix: 3 unique match(es)
// closePrint: 4 unique match(es)
// printData: 1 unique match(es)
// CMD_INQUIRY_STA: 1 unique match(es)
// SendCmd: 1 unique match(es)
// SendCmdTwo: 1 unique match(es)
// BulkWrite: 1 unique match(es)
// BulkWriteType: 1 unique match(es)
// returnCmd: 1 unique match(es)
// connectHIDDevice: 3 unique match(es)
// connectHIDDeviceForId: 1 unique match(es)
// hidDeviceOpen: 2 unique match(es)
// hidDeviceClose: 1 unique match(es)
// getRfidData: 1 unique match(es)
// createPrintData: 1 unique match(es)
// autoConnectDevice: 1 unique match(es)
// connectDevice: 1 unique match(es)
// waitForConnectDevice: 1 unique match(es)
// loopWaitForConnectDevice: 1 unique match(es)
// getLzma: 1 unique match(es)
// initPrinter: 1 unique match(es)
// getPrinterState: 1 unique match(es)
// doSupVanPrint: 1 unique match(es)
// USB_VID_PID: 19 unique match(es)
// getDevices_call: 2 unique match(es)
// Total unique identifiers found: 29
